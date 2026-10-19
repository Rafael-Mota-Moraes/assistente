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

/// Estado da VM pelo `incus list` (a VM é criada por `bin/assistente vm criar`).
#[tauri::command]
fn vm_status() -> VmStatus {
    let name = load_config()
        .map(|c| c.vm.name)
        .unwrap_or_else(|_| "assistente".to_owned());
    let output = match Command::new("incus")
        .args(["list", &format!("^{name}$"), "--format", "json"])
        .output()
    {
        Ok(output) => output,
        Err(_) => {
            return VmStatus {
                state: VmState::NoIncus,
                detail: Some("incus não instalado".into()),
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
            open_in_cursor
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Assistente");
}
