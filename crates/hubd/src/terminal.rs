//! Terminal de uma sessão por WebSocket (F2.3): cada conexão é um `tmux attach` num PTY.
//! Fechar a conexão só desanexa; a sessão continua no tmux.
//!
//! Protocolo (spec 02):
//! - binário, nos dois sentidos: bytes do terminal;
//! - texto UI → hubd: `{"type":"input","data":"…"}` ou `{"type":"resize","cols":120,"rows":40}`;
//! - texto hubd → UI: `{"type":"exit"}` quando o tmux attach termina (sessão encerrada) e
//!   `{"type":"exit","status":N}` quando o processo da sessão morreu (F14.3).

use crate::sessions::{PaneState, Tmux};
use axum::extract::ws::{Message, WebSocket};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::io::{Read, Write};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// De quanto em quanto tempo o terminal pergunta ao tmux se o processo da sessão ainda vive.
const VERIFICA: Duration = Duration::from_millis(500);
/// Espera antes da primeira pergunta: dá tempo de a saída chegar antes do aviso de fim.
const PRIMEIRA: Duration = Duration::from_millis(250);

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Control {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

pub async fn run(mut socket: WebSocket, tmux: Tmux, id: String) {
    if let Err(e) = bridge(&mut socket, &tmux, &id).await {
        warn!("terminal {id}: {e}");
        let _ = socket
            .send(Message::Text(
                serde_json::json!({ "type": "error", "message": e })
                    .to_string()
                    .into(),
            ))
            .await;
    }
    let _ = socket.send(Message::Close(None)).await;
}

async fn bridge(socket: &mut WebSocket, tmux: &Tmux, id: &str) -> Result<(), String> {
    // F14.3: a sessão já morreu antes do attach. O tmux rola o painel uma linha quando o
    // processo morre, então a tela do painel morto perde a primeira linha da saída. O
    // terminal manda o que o processo imprimiu (o histórico do painel) e o status de saída,
    // sem abrir terminal: a aba mostra a saída e o fim.
    if let PaneState::Dead(status) = tmux.pane_state(id).await {
        if let Some(saida) = tmux.capture(id).await {
            let _ = socket.send(Message::Binary(saida.into())).await;
        }
        let _ = socket.send(Message::Text(fim(status).into())).await;
        return Ok(());
    }
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;
    let mut command = CommandBuilder::new("tmux");
    // `-u`: o hubd roda sem locale, e sem UTF-8 o cliente desenha acentos como `_`.
    command.arg("-u");
    for arg in tmux.std_args() {
        command.arg(arg);
    }
    // `set-clipboard` também aqui: sessões criadas por um hubd antigo passam a copiar por OSC 52.
    command.args(["set-option", "-s", "set-clipboard", "on", ";"]);
    command.args(["attach-session", "-t", &format!("={id}")]);
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| format!("tmux attach: {e}"))?;
    drop(pair.slave);

    // Leitura do PTY é bloqueante: thread própria → canal → WebSocket.
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    // Escrita também é bloqueante: outra thread.
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        while let Ok(bytes) = in_rx.recv() {
            if writer
                .write_all(&bytes)
                .and_then(|_| writer.flush())
                .is_err()
            {
                break;
            }
        }
    });

    // F14.3: o painel da sessão fica na tela quando o processo morre (`remain-on-exit`), então
    // o attach não termina: o terminal pergunta ao tmux se o processo ainda vive e avisa a
    // UI com o status de saída.
    let mut verificacao =
        tokio::time::interval_at(tokio::time::Instant::now() + PRIMEIRA, VERIFICA);
    // Com saída correndo, o tick atrasa: uma pergunta por vez, sem acumular.
    verificacao.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut encerrada = false;

    loop {
        tokio::select! {
            output = out_rx.recv() => match output {
                Some(bytes) => {
                    if socket.send(Message::Binary(bytes.into())).await.is_err() {
                        break;
                    }
                }
                None => {
                    // PTY fechou: o attach terminou (sessão encerrada ou servidor tmux caiu).
                    let _ = socket
                        .send(Message::Text(r#"{"type":"exit"}"#.into()))
                        .await;
                    break;
                }
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Binary(bytes))) => {
                    let _ = in_tx.send(bytes.to_vec());
                }
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<Control>(&text) {
                    Ok(Control::Input { data }) => {
                        let _ = in_tx.send(data.into_bytes());
                    }
                    Ok(Control::Resize { cols, rows }) => {
                        let size = PtySize { rows, cols, pixel_width: 0, pixel_height: 0 };
                        if let Err(e) = pair.master.resize(size) {
                            debug!("resize {id}: {e}");
                        }
                    }
                    Err(e) => debug!("mensagem de controle inválida em {id}: {e}"),
                },
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
            _ = verificacao.tick(), if !encerrada => {
                if let PaneState::Dead(status) = tmux.pane_state(id).await {
                    let _ = socket.send(Message::Text(fim(status).into())).await;
                    encerrada = true;
                }
            }
        }
    }
    // Só o cliente `tmux attach` morre; a sessão segue no servidor tmux.
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

/// O aviso de fim para a UI (F14.3): com o código de saída quando o tmux sabe qual é.
fn fim(status: Option<i32>) -> String {
    match status {
        Some(status) => serde_json::json!({ "type": "exit", "status": status }).to_string(),
        None => r#"{"type":"exit"}"#.to_owned(),
    }
}
