//! hubd: o processo que roda dentro da VM e que o app Tauri controla.
//! Spec: docs/specs/02-hubd-e-sessoes.md

mod api;
mod sessions;
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
    let state = api::AppState::new(config, home, token, tunnel, sessions::Tmux::default());
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("não consegui escutar em {addr}"))?;
    info!(
        "hubd ouvindo em http://{addr} (config: {})",
        config_path.display()
    );
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
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
