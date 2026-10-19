//! Túnel SSH até o notebook (F4.2): `ssh -N -L …` como processo filho, com reinício e backoff.
//! Dentro da VM, o 9router do notebook aparece em 127.0.0.1 (memória é local, F5.0).
//! Spec: docs/specs/07-notebook-e-9router.md

use hub_core::config::NotebookConfig;
use serde::Serialize;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::{info, warn};

/// Rodando há mais que isso sem cair = conexão boa; o backoff volta ao mínimo.
const STABLE_AFTER: Duration = Duration::from_secs(60);
/// O `ssh` não avisa quando os -L ficaram prontos; vivo depois disso conta como "up".
const UP_AFTER: Duration = Duration::from_secs(3);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelState {
    Connecting,
    Up,
    Down,
}

#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatus {
    pub state: TunnelState,
    /// Última linha que o `ssh` escreveu no stderr (ex.: "Address already in use").
    pub last_error: Option<String>,
    pub restarts: u32,
    /// Momento da última mudança de estado (segundos Unix).
    pub since: u64,
}

#[derive(Clone)]
pub struct TunnelHandle(Arc<Mutex<TunnelStatus>>);

impl TunnelHandle {
    pub fn status(&self) -> TunnelStatus {
        self.0.lock().expect("estado do túnel").clone()
    }

    fn update(&self, f: impl FnOnce(&mut TunnelStatus)) {
        let mut status = self.0.lock().expect("estado do túnel");
        let before = status.state;
        f(&mut status);
        if status.state != before {
            status.since = now();
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Linha de comando do `ssh`; separada para testar sem rede.
pub fn ssh_argv(notebook: &NotebookConfig) -> Vec<String> {
    let mut argv: Vec<String> = [
        "ssh",
        "-N",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "BatchMode=yes",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "StrictHostKeyChecking=accept-new",
    ]
    .map(str::to_owned)
    .into();
    for (name, &port) in &notebook.tunnel {
        if name == "ai_memory" {
            continue;
        }
        argv.push("-L".to_owned());
        argv.push(format!("127.0.0.1:{port}:127.0.0.1:{port}"));
    }
    argv.push(notebook.ssh_host.clone());
    argv
}

/// Inicia o laço do túnel em segundo plano e devolve o estado compartilhado.
pub fn spawn(notebook: NotebookConfig) -> TunnelHandle {
    let handle = TunnelHandle(Arc::new(Mutex::new(TunnelStatus {
        state: TunnelState::Connecting,
        last_error: None,
        restarts: 0,
        since: now(),
    })));
    let task_handle = handle.clone();
    tokio::spawn(async move { run(notebook, task_handle).await });
    handle
}

async fn run(notebook: NotebookConfig, handle: TunnelHandle) {
    let argv = ssh_argv(&notebook);
    let mut backoff = Duration::from_secs(1);
    loop {
        handle.update(|s| s.state = TunnelState::Connecting);
        info!("túnel: {}", argv.join(" "));
        let started = Instant::now();
        let exit = match Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(mut child) => {
                let stderr = child.stderr.take().map(|e| BufReader::new(e).lines());
                let errors = handle.clone();
                let reader = tokio::spawn(async move {
                    let Some(mut lines) = stderr else { return };
                    while let Ok(Some(line)) = lines.next_line().await {
                        warn!("túnel ssh: {line}");
                        errors.update(|s| s.last_error = Some(line));
                    }
                });
                let marker = handle.clone();
                let up = tokio::spawn(async move {
                    tokio::time::sleep(UP_AFTER).await;
                    marker.update(|s| s.state = TunnelState::Up);
                });
                let exit = child.wait().await;
                up.abort();
                let _ = reader.await;
                exit.map(|s| s.to_string())
                    .unwrap_or_else(|e| e.to_string())
            }
            Err(e) => {
                handle.update(|s| s.last_error = Some(format!("não consegui rodar ssh: {e}")));
                "falha ao iniciar".to_owned()
            }
        };
        if started.elapsed() > STABLE_AFTER {
            backoff = Duration::from_secs(1);
        }
        handle.update(|s| {
            s.state = TunnelState::Down;
            s.restarts += 1;
        });
        warn!(
            "túnel caiu ({exit}); nova tentativa em {}s",
            backoff.as_secs()
        );
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hub_core::Config;
    use std::path::Path;

    #[test]
    fn argv_forwards_every_tunnel_port_on_loopback() {
        let config = Config::parse(
            include_str!("../../../config/assistente.exemplo.toml"),
            Path::new("/home/rafa"),
        )
        .unwrap();
        let argv = ssh_argv(&config.notebook);
        assert_eq!(argv.first().unwrap(), "ssh");
        assert_eq!(argv.last().unwrap(), "notebook");
        assert!(argv.contains(&"ExitOnForwardFailure=yes".to_owned()));
        assert!(argv.contains(&"BatchMode=yes".to_owned()));
        let forwards: Vec<_> = argv
            .windows(2)
            .filter(|w| w[0] == "-L")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(forwards, ["127.0.0.1:20128:127.0.0.1:20128"]);
    }
}
