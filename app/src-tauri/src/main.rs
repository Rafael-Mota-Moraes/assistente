//! Casca desktop do Assistente: a UI fala direto com o hubd (HTTP/WS). Aqui só ficam as
//! coisas que precisam do HOST: ler a config local, ver o estado da VM e abrir o Cursor.
//! Spec: docs/specs/03-interface.md

// Sem janela de console extra no Windows em release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use hub_core::Config;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME não definido".to_owned())
}

fn load_config() -> Result<Config, String> {
    let home = home()?;
    let path = home.join(".config/assistente/assistente.toml");
    Config::load(&path, &home).map_err(|e| format!("{e} — rode ./instalar.sh"))
}

#[derive(Serialize)]
struct HubConnection {
    url: String,
    token: Option<String>,
}

/// Onde está o hubd. `ASSISTENTE_HUB_URL` sobrescreve (desenvolvimento com `just hubd`).
#[tauri::command]
fn hub_connection() -> Result<HubConnection, String> {
    let config = load_config();
    if let Ok(url) = std::env::var("ASSISTENTE_HUB_URL") {
        let token = config
            .ok()
            .and_then(|c| std::fs::read_to_string(&c.hub.token_file).ok())
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty());
        return Ok(HubConnection { url, token });
    }
    let config = config?;
    let port = config
        .hub
        .listen
        .rsplit(':')
        .next()
        .unwrap_or("7878")
        .to_owned();
    let token = std::fs::read_to_string(&config.hub.token_file)
        .map(|t| t.trim().to_owned())
        .map_err(|e| format!("token do hub ({}): {e}", config.hub.token_file.display()))?;
    Ok(HubConnection {
        url: format!("http://{}:{port}", config.vm.ip),
        token: Some(token),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum VmState {
    Running,
    Stopped,
    Missing,
    NoIncus,
    Unknown,
}

#[derive(Serialize)]
struct VmStatus {
    state: VmState,
    detail: Option<String>,
}

fn vm_name() -> String {
    load_config()
        .map(|c| c.vm.name)
        .unwrap_or_else(|_| "assistente".to_owned())
}

fn run_incus(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("incus")
        .args(args)
        .output()
        .map_err(|_| "incus não instalado".to_owned())
}

fn query_vm_status(name: &str) -> VmStatus {
    let output = match run_incus(&["list", &format!("^{name}$"), "--format", "json"]) {
        Ok(output) => output,
        Err(detail) => {
            return VmStatus {
                state: VmState::NoIncus,
                detail: Some(detail),
            };
        }
    };
    if !output.status.success() {
        return VmStatus {
            state: VmState::Unknown,
            detail: Some(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        };
    }
    let list: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap_or_default();
    let state = match list.first().and_then(|vm| vm["status"].as_str()) {
        None => VmState::Missing,
        Some("Running") => VmState::Running,
        Some("Stopped") => VmState::Stopped,
        Some(_) => VmState::Unknown,
    };
    VmStatus {
        state,
        detail: None,
    }
}

/// Estado da VM pelo `incus list` (a VM é criada por `bin/assistente vm criar`).
#[tauri::command]
fn vm_status() -> VmStatus {
    query_vm_status(&vm_name())
}

/// Liga a VM (`incus start`), idempotente se já estiver de pé.
#[tauri::command]
fn vm_start() -> Result<(), String> {
    let name = vm_name();
    match query_vm_status(&name).state {
        VmState::Running => return Ok(()),
        VmState::Missing => {
            return Err("VM não criada — rode bin/assistente vm criar".into());
        }
        VmState::NoIncus => {
            return Err("incus não instalado".into());
        }
        VmState::Stopped | VmState::Unknown => {}
    }
    let output = run_incus(&["start", &name])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Desliga a VM (`incus stop`), idempotente se já estiver parada ou ausente.
#[tauri::command]
fn vm_stop() -> Result<(), String> {
    let name = vm_name();
    match query_vm_status(&name).state {
        VmState::Stopped | VmState::Missing => return Ok(()),
        VmState::NoIncus => return Err("incus não instalado".into()),
        VmState::Running | VmState::Unknown => {}
    }
    let output = run_incus(&["stop", &name])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Abre um projeto no Cursor do host. O caminho precisa estar dentro de uma raiz de perfil
/// (a VM monta essas raízes no mesmo caminho, então o caminho do hubd vale aqui).
#[tauri::command]
fn open_in_cursor(path: String) -> Result<(), String> {
    let config = load_config()?;
    let not_a_project = || format!("{path} não é um projeto de nenhum perfil");
    // canonicalize resolve `..` e links: "raiz/../../.ssh" não passa.
    let real = std::fs::canonicalize(Path::new(&path)).map_err(|_| not_a_project())?;
    let inside_profile = config.profiles.values().any(|p| {
        std::fs::canonicalize(&p.root).is_ok_and(|root| real.starts_with(&root) && real != root)
    });
    if !inside_profile || !real.is_dir() {
        return Err(not_a_project());
    }
    Command::new("cursor")
        .arg(&real)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir o Cursor: {e}"))
}

/// Abre uma URL no navegador padrão do host (links clicáveis do terminal, F2.4).
/// Só http/https: o terminal não pode pedir file://, javascript: etc.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let allowed = ["http://", "https://"]
        .iter()
        .any(|scheme| url.to_ascii_lowercase().starts_with(scheme));
    if !allowed || url.chars().any(|c| c.is_control()) {
        return Err(format!("URL não permitida: {url}"));
    }
    Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir o navegador: {e}"))
}

fn main() {
    tauri::Builder::default()
        // HTTP pelo lado Rust: o WebKit bloqueia fetch de tauri://localhost para http://<ip da VM>
        // (mixed content). Escopo liberado em capabilities/default.json.
        .plugin(tauri_plugin_http::init())
        // Mesmo motivo para o WebSocket dos terminais (ws:// a partir de tauri://localhost).
        .plugin(tauri_plugin_websocket::init())
        // Copiar/colar do terminal: o navigator.clipboard do WebKitGTK falha em silêncio.
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            hub_connection,
            vm_status,
            vm_start,
            vm_stop,
            open_in_cursor,
            open_url
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Assistente");
}
