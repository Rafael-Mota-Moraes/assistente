//! hubd: o processo que roda dentro da VM e que o app Tauri controla.
//! Spec: docs/specs/02-hubd-e-sessoes.md

mod api;
mod exec;
mod kanban;
mod memory;
mod sessions;
mod sleep;
mod terminal;
mod tunnel;

use anyhow::{Context, bail};
use clap::Parser;
use hub_core::{Config, expand_home};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "hubd",
    version,
    about = "Hub do Assistente: sessões de agentes, perfis, notebook e modo dormir"
)]
struct Args {
    /// Arquivo de configuração (exemplo comentado em config/assistente.exemplo.toml)
    #[arg(long, default_value = "~/.config/assistente/assistente.toml")]
    config: PathBuf,
    /// Sobrescreve `hub.listen` (ex.: 127.0.0.1:7878 para desenvolver fora da VM)
    #[arg(long)]
    listen: Option<String>,
    /// Não abre o túnel SSH até o notebook (desenvolvimento fora da VM)
    #[arg(long)]
    no_tunnel: bool,
    /// Pasta com o build da UI servido em /ui/ (F8.3); padrão: a versão em uso na VM
    #[arg(long, default_value = "~/.assistente/versoes/atual/ui")]
    ui_dir: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("hubd=info")),
        )
        .init();

    let args = Args::parse();
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME não definido")?);
    let config_path = expand_home(&args.config, &home);
    let config = Config::load(&config_path, &home)?;
    let listen = args.listen.unwrap_or_else(|| config.hub.listen.clone());
    let addr: SocketAddr = listen
        .parse()
        .with_context(|| format!("endereço inválido: {listen}"))?;
    let token = read_token(&config.hub.token_file, addr)?;

    let tunnel = (!args.no_tunnel).then(|| tunnel::spawn(config.notebook.clone()));
    let ui_dir = expand_home(&args.ui_dir, &home);
    if !ui_dir.is_dir() {
        warn!(
            "{} não existe; /ui/ responde 404 (just build-ui gera o conteúdo)",
            ui_dir.display()
        );
    }
    // API dos agentes (F9.5): só loopback (validado na config). Sem ela o hubd funciona, mas
    // as sessões nascem sem poder pedir nada fora do jail.
    let agent_addr = config.hub.agent_listen.clone();
    let agent_listener = match tokio::net::TcpListener::bind(&agent_addr).await {
        Ok(listener) => Some(listener),
        Err(e) => {
            warn!("API dos agentes desligada: não consegui escutar em {agent_addr}: {e}");
            None
        }
    };
    let exec_port = agent_listener
        .as_ref()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port());
    let state = api::AppState::new(config, home, token, tunnel, sessions::Tmux::default())
        .with_ui_dir(ui_dir.is_dir().then_some(ui_dir))
        .with_exec_port(exec_port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("não consegui escutar em {addr}"))?;
    info!(
        "hubd ouvindo em http://{addr} (config: {})",
        config_path.display()
    );

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let stopped = |mut rx: tokio::sync::watch::Receiver<bool>| async move {
        let _ = rx.wait_for(|stop| *stop).await;
    };
    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = stop_tx.send(true);
    });
    let agents = agent_listener.map(|listener| {
        info!("API dos agentes em http://{agent_addr}");
        let router = api::agent_router(state.clone());
        let stop = stopped(stop_rx.clone());
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router)
                .with_graceful_shutdown(stop)
                .await
            {
                warn!("API dos agentes parou: {e}");
            }
        })
    });
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(stopped(stop_rx))
        .await?;
    if let Some(agents) = agents {
        let _ = agents.await;
    }
    Ok(())
}

/// Sem token só é aceito em loopback (desenvolvimento com `just hubd`).
fn read_token(path: &Path, addr: SocketAddr) -> anyhow::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => Ok(Some(text.trim().to_owned())),
        Ok(_) | Err(_) if addr.ip().is_loopback() => {
            warn!(
                "sem token em {}: API aberta (aceito só porque escuta em loopback)",
                path.display()
            );
            Ok(None)
        }
        Ok(_) => bail!("token vazio em {}", path.display()),
        Err(e) => Err(e).with_context(|| {
            format!(
                "sem token em {} (gere com `bin/assistente token`)",
                path.display()
            )
        }),
    }
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("sinal SIGTERM");
    tokio::select! {
        _ = ctrl_c => {},
        _ = term.recv() => {},
    }
    info!("hubd encerrando");
}
