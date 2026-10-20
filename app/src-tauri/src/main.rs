//! Casca desktop do Assistente: a UI fala direto com o hubd (HTTP/WS). Aqui só ficam as
//! coisas que precisam do HOST: ler a config local, ver o estado da VM, abrir o Cursor e
//! ancorar a webview da memória na janela principal.
//! Spec: docs/specs/03-interface.md

// Sem janela de console extra no Windows em release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use hub_core::Config;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::Manager;

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

/// Rótulo da webview filha com a web UI do ai-memory, ancorada na área principal (F5.5).
const MEMORY_WEBVIEW: &str = "memoria";

/// Valida a URL da memória: só http/https e sem caracteres de controle.
fn memory_url(url: &str) -> Result<tauri::Url, String> {
    let parsed: tauri::Url = url.parse().map_err(|e| format!("URL inválida: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || url.chars().any(|c| c.is_control()) {
        return Err(format!("URL não permitida: {url}"));
    }
    Ok(parsed)
}

/// Retângulo em pixels lógicos (o mesmo que px de CSS), vindo da UI.
#[derive(Serialize, Deserialize)]
struct PanelRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl PanelRect {
    fn bounds(self) -> tauri::Rect {
        tauri::Rect {
            position: tauri::LogicalPosition::new(self.x, self.y).into(),
            size: tauri::LogicalSize::new(self.width, self.height).into(),
        }
    }
}

/// Aloca a web UI do ai-memory DENTRO da janela principal: uma webview filha cobre a área
/// de conteúdo, como um iframe mas fora do processo da UI. No topo, a origem é a do hubd,
/// então o cookie de sessão não é de terceiros e o WebKitGTK o envia (iframe não funciona).
/// O chrome do painel (título/fechar) é desenhado pela React; a webview só preenche o retângulo.
#[tauri::command]
async fn open_memory_panel(
    app: tauri::AppHandle,
    url: String,
    rect: PanelRect,
) -> Result<(), String> {
    let url = memory_url(&url)?;
    let window = app
        .get_window("main")
        .ok_or_else(|| "janela principal não encontrada".to_owned())?;
    if let Some(webview) = app.get_webview(MEMORY_WEBVIEW) {
        webview.navigate(url).map_err(|e| e.to_string())?;
        webview
            .set_bounds(rect.bounds())
            .map_err(|e| e.to_string())?;
        return webview.show().map_err(|e| e.to_string());
    }
    let bounds = rect.bounds();
    window
        .add_child(
            tauri::webview::WebviewBuilder::new(MEMORY_WEBVIEW, tauri::WebviewUrl::External(url)),
            bounds.position,
            bounds.size,
        )
        .map(|_| ())
        .map_err(|e| format!("não consegui alocar o painel da memória: {e}"))
}

/// Acompanha o retângulo do âncora (janela redimensionada, layout mudou).
#[tauri::command]
async fn update_memory_panel_bounds(app: tauri::AppHandle, rect: PanelRect) -> Result<(), String> {
    if let Some(webview) = app.get_webview(MEMORY_WEBVIEW) {
        webview
            .set_bounds(rect.bounds())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Fecha a webview filha (o painel React também sai do DOM).
#[tauri::command]
async fn close_memory_panel(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(webview) = app.get_webview(MEMORY_WEBVIEW) {
        webview.close().map_err(|e| e.to_string())?;
    }
    Ok(())
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
            open_url,
            open_memory_panel,
            update_memory_panel_bounds,
            close_memory_panel
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Assistente");
}
