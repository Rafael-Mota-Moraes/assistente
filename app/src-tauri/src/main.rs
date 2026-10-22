//! Casca desktop do Assistente: a UI fala direto com o hubd (HTTP/WS). Aqui só ficam as
//! coisas que precisam do HOST: ler a config local, ver o estado da VM, abrir o Cursor,
//! ancorar a webview da memória na janela principal e rodar no PC o pedido que o agente
//! fez e você aprovou (F9.10).
//! Spec: docs/specs/03-interface.md

// Sem janela de console extra no Windows em release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use http_body_util::BodyExt;
use hub_core::Config;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::Manager;

mod pick;

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
/// Async + spawn_blocking: o `incus` pode demorar e não pode travar a thread da janela.
#[tauri::command]
async fn vm_status() -> VmStatus {
    tauri::async_runtime::spawn_blocking(|| query_vm_status(&vm_name()))
        .await
        .unwrap_or_else(|e| VmStatus {
            state: VmState::Unknown,
            detail: Some(e.to_string()),
        })
}

/// Liga a VM (`incus start`), idempotente se já estiver de pé.
#[tauri::command]
async fn vm_start() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(start_vm)
        .await
        .map_err(|e| e.to_string())?
}

fn start_vm() -> Result<(), String> {
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
async fn vm_stop() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(stop_vm)
        .await
        .map_err(|e| e.to_string())?
}

fn stop_vm() -> Result<(), String> {
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
    if !inside_profile_root(&config, &real) || !real.is_dir() {
        return Err(not_a_project());
    }
    Command::new("cursor")
        .arg(&real)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("não consegui abrir o Cursor: {e}"))
}

/// O caminho está dentro de uma raiz de perfil? (canonicalize resolve `..` e links:
/// "raiz/../../.ssh" não passa.)
fn inside_profile_root(config: &Config, path: &Path) -> bool {
    let Ok(real) = std::fs::canonicalize(path) else {
        return false;
    };
    config.profiles.values().any(|p| {
        std::fs::canonicalize(&p.root).is_ok_and(|root| real.starts_with(&root) && real != root)
    })
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

// ---- Pedido no PC (F9.10) ----
//
// O agente pede (`fora-do-jail --pc`), o hubd guarda o pedido e **não** executa nada na
// VM. Quem executa no PC é o app, e só depois que você aprovou no painel de pedidos.
// O app pergunta ao hubd o que foi aprovado e roda exatamente aquilo: nem o comando nem a
// pasta saem da UI, e o resultado volta ao hubd (`POST /api/exec/{id}/result`).

/// O que o hubd guardou do pedido (o app roda exatamente isto).
#[derive(Deserialize)]
struct ExecView {
    /// `host` = pedido para o PC (F9.9).
    #[serde(default)]
    target: String,
    /// `running` = aprovado e esperando o app rodar.
    #[serde(default)]
    state: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    timeout_secs: u64,
    /// Pedido `host` já foi pego pelo app para execução no PC (F9.11).
    #[serde(default)]
    picked_up: bool,
}

/// O que o app devolve ao hubd depois de rodar no PC.
#[derive(Serialize, Clone)]
struct HostExecResult {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    truncated: bool,
    timed_out: bool,
    duration_ms: u64,
}

/// Teto da saída guardada por stream (fica o fim, que é onde costuma estar o erro).
const MAX_HOST_OUTPUT: usize = 64 * 1024;

/// URL e token do hubd (o app pergunta ao hubd o que foi aprovado, F9.10).
fn hub_url_and_token(config: &Config) -> Result<(String, String), String> {
    let token = std::fs::read_to_string(&config.hub.token_file)
        .map_err(|e| format!("token do hub ({}): {e}", config.hub.token_file.display()))?
        .trim()
        .to_owned();
    let url = match std::env::var("ASSISTENTE_HUB_URL") {
        Ok(url) => url,
        Err(_) => {
            let port = config.hub.listen.rsplit(':').next().unwrap_or("7878");
            format!("http://{}:{port}", config.vm.ip)
        }
    };
    Ok((url, token))
}

/// GET no hubd (o app não inventa comando: pergunta o que foi aprovado).
async fn hub_get(url: &str, token: &str, path: &str) -> Result<serde_json::Value, String> {
    let parsed = tauri::Url::parse(url).map_err(|e| format!("URL do hubd inválida: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL do hubd sem host: {url}"))?
        .to_owned();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| format!("URL do hubd sem porta: {url}"))?;
    let stream = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| format!("não consegui falar com o hubd em {url}: {e}"))?;
    let io = hyper_util::rt::TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| format!("falha na conversa com o hubd: {e}"))?;
    tauri::async_runtime::spawn(async move {
        let _ = conn.await;
    });
    let request = hyper::Request::builder()
        .method("GET")
        .uri(path)
        .header(hyper::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(http_body_util::Empty::<hyper::body::Bytes>::new())
        .map_err(|e| format!("requisição inválida: {e}"))?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|e| format!("o hubd não respondeu: {e}"))?;
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|e| format!("resposta do hubd ilegível: {e}"))?
        .to_bytes();
    if !status.is_success() {
        return Err(format!("o hubd respondeu {status}"));
    }
    serde_json::from_slice(&body).map_err(|e| format!("resposta do hubd ilegível: {e}"))
}

/// O id do pedido vem da UI: só o id do hubd (hex) vale, para não virar outra rota.
fn valid_request_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Comando do app: roda no PC o pedido `host` que você aprovou (F9.10/F9.11).
#[tauri::command]
async fn host_exec(id: String) -> Result<HostExecResult, String> {
    if !valid_request_id(&id) {
        return Err("id de pedido inválido".into());
    }
    let config = load_config()?;
    let (url, token) = hub_url_and_token(&config)?;
    // Pega o pedido no hubd (marca como pego para evitar execução dupla, F9.11).
    let value = hub_post(&url, &token, &format!("/api/exec/{id}/claim"), None).await?;
    let claim: ClaimResponse =
        serde_json::from_value(value).map_err(|e| format!("resposta de claim ilegível: {e}"))?;
    // Verificações de segurança.
    if claim.command.is_empty() {
        return Err("comando vazio".into());
    }
    let cwd = PathBuf::from(&claim.cwd);
    if !inside_profile_root(&config, &cwd) {
        return Err(format!(
            "{} não está dentro de uma raiz de perfil",
            claim.cwd
        ));
    }
    let command = claim.command.clone();
    let timeout = Duration::from_secs(claim.timeout_secs.clamp(1, 3600));
    let result = tauri::async_runtime::spawn_blocking(move || run_on_host(&cwd, &command, timeout))
        .await
        .map_err(|e| format!("a execução no PC falhou: {e}"))?;
    // Registra no log local do PC (F9.11).
    log_pc_exec(&id, &claim.cwd, &command, &result).await;
    Ok(result)
}

#[derive(Deserialize)]
struct ClaimResponse {
    id: String,
    command: String,
    cwd: String,
    env: Vec<(String, String)>,
    timeout_secs: u64,
}

/// POST no hubd (usado para claim).
async fn hub_post(
    url: &str,
    token: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let parsed = tauri::Url::parse(url).map_err(|e| format!("URL do hubd inválida: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL do hubd sem host: {url}"))?
        .to_owned();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| format!("URL do hubd sem porta: {url}"))?;
    let stream = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| format!("não consegui falar com o hubd em {url}: {e}"))?;
    let io = hyper_util::rt::TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
        .await
        .map_err(|e| format!("falha na conversa com o hubd: {e}"))?;
    tauri::async_runtime::spawn(async move {
        let _ = conn.await;
    });
    let mut request = hyper::Request::builder()
        .method("POST")
        .uri(path)
        .header(hyper::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(hyper::header::CONTENT_TYPE, "application/json");
    if let Some(body) = body {
        request = request.body(http_body_util::Full::new(http_body_util::Bytes::from(
            body.to_string(),
        )));
    } else {
        request = request.body(http_body_util::Empty::<hyper::body::Bytes>::new());
    }
    let request = request.map_err(|e| format!("requisição inválida: {e}"))?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|e| format!("o hubd não respondeu: {e}"))?;
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|e| format!("resposta do hubd ilegível: {e}"))?
        .to_bytes();
    if !status.is_success() {
        return Err(format!("o hubd respondeu {status}"));
    }
    serde_json::from_slice(&body).map_err(|e| format!("resposta do hubd ilegível: {e}"))
}

/// Roda `bash -lc <comando>` no PC: na pasta do projeto, com timeout e saída truncada.
fn run_on_host(cwd: &Path, command: &str, timeout: Duration) -> Result<HostExecResult, String> {
    let started = Instant::now();
    let mut process = Command::new("bash");
    process
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Grupo próprio: no timeout, o que o comando deixou em segundo plano morre junto.
    process.process_group(0);
    // Sem prompt de credencial: `git push` sem chave não fica esperando senha.
    process.env("GIT_TERMINAL_PROMPT", "0");
    let mut child = process
        .spawn()
        .map_err(|e| format!("não consegui iniciar o bash: {e}"))?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (out_tx, out_rx) = mpsc::channel::<(String, bool)>();
    let (err_tx, err_rx) = mpsc::channel::<(String, bool)>();
    thread::spawn(move || {
        let _ = out_tx.send(read_tail(stdout, MAX_HOST_OUTPUT));
    });
    thread::spawn(move || {
        let _ = err_tx.send(read_tail(stderr, MAX_HOST_OUTPUT));
    });
    let (status_tx, status_rx) = mpsc::channel::<std::io::Result<std::process::ExitStatus>>();
    thread::spawn(move || {
        let _ = status_tx.send(child.wait());
    });
    let (exit_code, timed_out) = match status_rx.recv_timeout(timeout) {
        Ok(status) => (status.ok().and_then(|s| s.code()), false),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            kill_group(pid);
            let status = status_rx
                .recv_timeout(Duration::from_secs(5))
                .ok()
                .and_then(|s| s.ok());
            (status.and_then(|s| s.code()), true)
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => (None, false),
    };
    // Um neto que fechou o grupo pode segurar o pipe: não espera para sempre.
    let out = out_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    let err = err_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    Ok(HostExecResult {
        exit_code,
        stdout: out.0,
        stderr: err.0,
        truncated: out.1 || err.1,
        timed_out,
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

/// Lê tudo e guarda só os últimos `max` bytes (em UTF-8 válido).
fn read_tail(stream: Option<impl Read + Send + 'static>, max: usize) -> (String, bool) {
    let Some(mut stream) = stream else {
        return (String::new(), false);
    };
    let mut kept: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                kept.extend_from_slice(&buf[..n]);
                if kept.len() > max {
                    kept.drain(..kept.len() - max);
                    truncated = true;
                }
            }
        }
    }
    (String::from_utf8_lossy(&kept).into_owned(), truncated)
}

/// Mata o grupo do comando (o timeout leva junto o que ficou em segundo plano).
fn kill_group(pid: u32) {
    let group = format!("-{pid}");
    for (signal, grace) in [("-TERM", Duration::from_secs(2)), ("-KILL", Duration::ZERO)] {
        let _ = Command::new("kill")
            .args([signal, "--", group.as_str()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        thread::sleep(grace);
    }
}

/// Registra a execução no PC no log local (F9.11): ~/.local/state/assistente/pc-exec.log
async fn log_pc_exec(
    request_id: &str,
    cwd: &str,
    command: &str,
    result: &HostExecResult,
) {
    let home = match home() {
        Ok(h) => h,
        Err(_) => return,
    };
    let log_dir = home.join(".local/state/assistente");
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        tracing::warn!("não consegui criar diretório de log: {e}");
        return;
    }
    let log_file = log_dir.join("pc-exec.log");
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
    let exit_code = result.exit_code.map_or("timeout/signal".to_string(), |c| c.to_string());
    let line = format!(
        "{} | id={} | cwd={} | exit={} | dur={}ms | truncated={} | cmd={}\n",
        timestamp,
        request_id,
        cwd,
        exit_code,
        result.duration_ms,
        result.truncated,
        command.replace('\n', " ")
    );
    if let Err(e) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()))
    {
        tracing::warn!("não consegui escrever no log pc-exec: {e}");
    }
}

/// Rótulo da webview filha com a web UI do ai-memory, ancorada na área principal (F5.5).
const MEMORY_WEBVIEW: &str = "memoria";

/// Rótulo da webview filha com o preview do projeto (F12.1).
const PREVIEW_WEBVIEW: &str = "preview";

/// Valida a URL da memória: só http/https e sem caracteres de controle.
fn memory_url(url: &str) -> Result<tauri::Url, String> {
    let parsed: tauri::Url = url.parse().map_err(|e| format!("URL inválida: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || url.chars().any(|c| c.is_control()) {
        return Err(format!("URL não permitida: {url}"));
    }
    Ok(parsed)
}

/// Valida a URL do preview (F12.1): só http/https, sem credenciais (o `projetos.toml`
/// ficaria com a senha) e sem caractere de controle. Mesma regra do hubd.
fn preview_url(url: &str) -> Result<tauri::Url, String> {
    hub_core::validate_preview_url(url)?;
    let parsed: tauri::Url = url
        .trim()
        .parse()
        .map_err(|e| format!("URL inválida: {e}"))?;
    Ok(parsed)
}

/// Retângulo em pixels lógicos (o mesmo que px de CSS), vindo da UI.
#[derive(Serialize, Deserialize, Clone, Copy)]
struct PanelRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl PanelRect {
    /// Converte px lógicos da UI (getBoundingClientRect, CSS) em px FÍSICOS da janela para
    /// o `add_child`. Físico de propósito: no X11 o wry reescala coordenadas lógicas com o
    /// DPI que a tela reporta (scale_factor_from_x11), que pode ser outro que o da janela;
    /// coordenada física passa direto, sem reescala (tauri#15656). No Linux isso é só para
    /// outras plataformas: o wry ignora os bounds do add_child na GtkBox e o posicionamento
    /// real fica com `gtk_panel::anchor` (abaixo).
    fn bounds(&self, scale: f64) -> tauri::Rect {
        let x = (self.x * scale).round() as i32;
        let y = (self.y * scale).round() as i32;
        let width = ((self.width * scale).round() as u32).max(1);
        let height = ((self.height * scale).round() as u32).max(1);
        tauri::Rect {
            position: tauri::PhysicalPosition::new(x, y).into(),
            size: tauri::PhysicalSize::new(width, height).into(),
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
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    if let Some(webview) = app.get_webview(MEMORY_WEBVIEW) {
        webview.navigate(url).map_err(|e| e.to_string())?;
        webview
            .set_bounds(rect.bounds(scale))
            .map_err(|e| e.to_string())?;
        anchor_panel(&app, MEMORY_WEBVIEW, rect)?;
        return webview.show().map_err(|e| e.to_string());
    }
    let bounds = rect.bounds(scale);
    let webview = window
        .add_child(
            tauri::webview::WebviewBuilder::new(MEMORY_WEBVIEW, tauri::WebviewUrl::External(url)),
            bounds.position,
            bounds.size,
        )
        .map_err(|e| format!("não consegui alocar o painel da memória: {e}"))?;
    webview
        .set_bounds(rect.bounds(scale))
        .map_err(|e| e.to_string())?;
    // No Linux o wry ignora os bounds acima (veja gtk_panel): quem posiciona o painel
    // sobre a área de conteúdo, na frente da webview principal, é o anchor da React.
    anchor_panel(&app, MEMORY_WEBVIEW, rect)?;
    Ok(())
}

/// Acompanha o retângulo do âncora (janela redimensionada, layout mudou).
#[tauri::command]
async fn update_memory_panel_bounds(app: tauri::AppHandle, rect: PanelRect) -> Result<(), String> {
    if app.get_webview(MEMORY_WEBVIEW).is_some() {
        anchor_panel(&app, MEMORY_WEBVIEW, rect)?;
    }
    Ok(())
}

/// Ancora uma webview filha no retângulo da React (painel da memória, F5.5; preview, F12.1).
#[cfg(target_os = "linux")]
fn anchor_panel(
    app: &tauri::AppHandle,
    label: &'static str,
    rect: PanelRect,
) -> Result<(), String> {
    let webview = app
        .get_webview(label)
        .ok_or_else(|| format!("webview {label} não encontrada"))?;
    webview
        .with_webview(move |platform| {
            if let Err(e) = gtk_panel::anchor(&platform.inner(), rect) {
                eprintln!("painel {label}: {e}");
            }
        })
        .map_err(|e| e.to_string())
}

/// Fora do Linux os bounds do `add_child`/`set_bounds` (px físicos) bastam.
#[cfg(not(target_os = "linux"))]
fn anchor_panel(app: &tauri::AppHandle, label: &str, rect: PanelRect) -> Result<(), String> {
    if let Some(webview) = app.get_webview(label) {
        let scale = app
            .get_window("main")
            .and_then(|w| w.scale_factor().ok())
            .unwrap_or(1.0);
        webview
            .set_bounds(rect.bounds(scale))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Ancoragem GTK do painel da memória (F5.5). No Linux o wry (0.57) coloca a webview do
/// `add_child` direto na GtkBox da janela (`pack_start`, dividindo a janela com a UI) e
/// ignora os bounds — eles só valem nos ramo GtkFixed/X11, que o tauri não usa aqui.
/// Então no setup a webview principal passa a morar num GtkOverlay com um GtkFixed como
/// palco sobreposto, e a webview da memória é movida para o palco, posicionada nas
/// coordenadas de CSS que a React mede (getBoundingClientRect): GTK e WebKit usam a
/// mesma escala do GDK, px de CSS de um lado vale do outro.
#[cfg(target_os = "linux")]
mod gtk_panel {
    use gtk::glib::{Cast, IsA};
    use gtk::prelude::*;

    /// No setup: tira a webview principal da GtkBox e a recoloca como filho base de um
    /// GtkOverlay que ocupa a janela toda; o palco GtkFixed fica sobre ela, pronto para
    /// receber o painel da memória.
    pub fn wrap_main_in_overlay<W: IsA<gtk::Widget>>(main: &W) -> Result<(), String> {
        let parent = main
            .parent()
            .and_then(|p| p.downcast::<gtk::Box>().ok())
            .ok_or_else(|| "pai da webview principal não é uma GtkBox".to_owned())?;
        let overlay = gtk::Overlay::new();
        parent.remove(main);
        overlay.add(main);
        // Palco: filho de overlay com halign/valign Fill (o padrão) cobre a área toda.
        // Pass-through: sem ele o palco captura o ponteiro e a UI principal para de receber
        // cliques; o painel da memória continua recebendo os seus, por ser filho do palco.
        let stage = gtk::Fixed::new();
        overlay.add_overlay(&stage);
        overlay.set_overlay_pass_through(&stage, true);
        parent.pack_start(&overlay, true, true, 0);
        overlay.show_all();
        Ok(())
    }

    /// Move a webview da memória para o palco e ancla no retângulo da React (px de CSS).
    /// Idempotente: reancorar é só `move_` + `set_size_request` no palco.
    pub fn anchor<W: IsA<gtk::Widget>>(child: &W, rect: super::PanelRect) -> Result<(), String> {
        let x = rect.x.round() as i32;
        let y = rect.y.round() as i32;
        let width = rect.width.round() as i32;
        let height = rect.height.round() as i32;
        match child.parent() {
            // Recém-criada (add_child): está na GtkBox da janela; troca pelo palco.
            Some(parent) if parent.is::<gtk::Box>() => {
                let box_ = parent
                    .downcast::<gtk::Box>()
                    .map_err(|_| "esperava uma GtkBox".to_owned())?;
                let stage = box_
                    .children()
                    .into_iter()
                    .find_map(|w| w.downcast::<gtk::Overlay>().ok())
                    .ok_or_else(|| {
                        "GtkOverlay da janela não encontrado (setup não rodou?)".to_owned()
                    })?
                    .children()
                    .into_iter()
                    .find_map(|w| w.downcast::<gtk::Fixed>().ok())
                    .ok_or_else(|| "palco GtkFixed não encontrado".to_owned())?;
                box_.remove(child);
                stage.put(child, x, y);
                child.set_size_request(width, height);
                child.show();
            }
            // Já é filha do palco (reabertura, resize da janela): só reposiciona.
            Some(parent) => {
                let stage = parent
                    .downcast::<gtk::Fixed>()
                    .map_err(|_| "esperava o palco GtkFixed".to_owned())?;
                child.set_size_request(width, height);
                stage.move_(child, x, y);
            }
            None => return Err("webview da memória sem pai GTK".to_owned()),
        }
        Ok(())
    }
}

/// Fecha a webview filha (o painel React também sai do DOM).
#[tauri::command]
async fn close_memory_panel(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(webview) = app.get_webview(MEMORY_WEBVIEW) {
        webview.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---- Preview do projeto (F12.1) ----
//
// Mesma ideia do painel da memória (F5.5): a webview filha preenche o retângulo que a
// React mede, e o chrome (URL, recarregar, apontar, fechar) é React. A webview do preview
// fica fora das capabilities: a página apontada não alcança os comandos do app.

/// Abre (ou recarrega) o preview do projeto numa webview filha, ancorada na área de
/// conteúdo (F12.1). Chamar de novo com a mesma URL recarrega a página.
#[tauri::command]
async fn open_preview_panel(
    app: tauri::AppHandle,
    url: String,
    rect: PanelRect,
) -> Result<(), String> {
    let url = preview_url(&url)?;
    let window = app
        .get_window("main")
        .ok_or_else(|| "janela principal não encontrada".to_owned())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    if let Some(webview) = app.get_webview(PREVIEW_WEBVIEW) {
        webview.navigate(url).map_err(|e| e.to_string())?;
        webview
            .set_bounds(rect.bounds(scale))
            .map_err(|e| e.to_string())?;
        anchor_panel(&app, PREVIEW_WEBVIEW, rect)?;
        return webview.show().map_err(|e| e.to_string());
    }
    let bounds = rect.bounds(scale);
    let webview = window
        .add_child(
            // O script do modo apontar (`pick.js`) entra pelo plugin `pick` (F12.2), em toda
            // webview; é ele que responde ao `preview_picker`.
            tauri::webview::WebviewBuilder::new(PREVIEW_WEBVIEW, tauri::WebviewUrl::External(url)),
            bounds.position,
            bounds.size,
        )
        .map_err(|e| format!("não consegui alocar o preview: {e}"))?;
    webview
        .set_bounds(rect.bounds(scale))
        .map_err(|e| e.to_string())?;
    // No Linux o wry ignora os bounds (veja gtk_panel): quem posiciona é o anchor da React.
    anchor_panel(&app, PREVIEW_WEBVIEW, rect)?;
    Ok(())
}

/// Acompanha o retângulo do âncora (janela redimensionada, layout mudou).
#[tauri::command]
async fn update_preview_panel_bounds(app: tauri::AppHandle, rect: PanelRect) -> Result<(), String> {
    if app.get_webview(PREVIEW_WEBVIEW).is_some() {
        anchor_panel(&app, PREVIEW_WEBVIEW, rect)?;
    }
    Ok(())
}

/// Esconde ou mostra a webview do preview (F12.1). A webview é um widget nativo por cima
/// da UI: um modal do app precisa escondê-la para ficar visível.
#[tauri::command]
async fn preview_panel_visible(app: tauri::AppHandle, visible: bool) -> Result<(), String> {
    let Some(webview) = app.get_webview(PREVIEW_WEBVIEW) else {
        return Ok(());
    };
    if visible {
        webview.show().map_err(|e| e.to_string())
    } else {
        webview.hide().map_err(|e| e.to_string())
    }
}

/// Liga ou desliga o modo "apontar" na página do preview (F12.1/F12.2, API do `pick.js`).
/// O comando fala com a página por `eval`, sem IPC: a página remota não vira porta de entrada.
#[tauri::command]
async fn preview_picker(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let webview = app
        .get_webview(PREVIEW_WEBVIEW)
        .ok_or_else(|| "preview não está aberto".to_owned())?;
    let script =
        format!("window.__assistentePick && window.__assistentePick.setEnabled({enabled});");
    webview.eval(script).map_err(|e| e.to_string())
}

/// Fecha a webview do preview (o painel React também sai do DOM).
#[tauri::command]
async fn close_preview_panel(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(webview) = app.get_webview(PREVIEW_WEBVIEW) {
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
        // Modo apontar (F12.2): injeta o script do pick na webview e intercepta a
        // navegação `assistente-pick:`, que traz o elemento escolhido na página.
        .plugin(pick::plugin())
        .invoke_handler(tauri::generate_handler![
            hub_connection,
            vm_status,
            vm_start,
            vm_stop,
            open_in_cursor,
            open_url,
            host_exec,
            open_memory_panel,
            update_memory_panel_bounds,
            close_memory_panel,
            open_preview_panel,
            update_preview_panel_bounds,
            preview_panel_visible,
            preview_picker,
            close_preview_panel
        ])
        .setup(|app| {
            // O painel da memória sobrepõe a UI: a webview principal precisa morar num
            // GtkOverlay com palco GtkFixed (no Linux o wry ignora os bounds do
            // add_child — veja gtk_panel).
            #[cfg(target_os = "linux")]
            if let Some(webview) = app.get_webview("main") {
                webview
                    .with_webview(|platform| {
                        if let Err(e) = gtk_panel::wrap_main_in_overlay(&platform.inner()) {
                            eprintln!("painel da memória: {e}");
                        }
                    })
                    .map_err(|e| format!("não consegui preparar a janela: {e}"))?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Assistente");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pasta de trabalho do teste (dentro do tmp do ambiente de desenvolvimento).
    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("assistente-pc-exec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// O id vem da UI: só o id do hubd (hex) vale, para não virar outra rota.
    #[test]
    fn id_invalido_nao_vira_outra_rota() {
        assert!(valid_request_id("a1b2c3d4e5f60718"));
        assert!(!valid_request_id(""));
        assert!(!valid_request_id("../../api/exec/outro"));
        assert!(!valid_request_id("a b"));
        assert!(!valid_request_id("id/result"));
    }

    /// F9.10: o comando roda na pasta do projeto e volta com a saída e o código.
    #[test]
    fn roda_na_pasta_e_devolve_saida_e_codigo() {
        let dir = temp_dir("pasta");
        let result =
            run_on_host(&dir, "pwd; echo aviso >&2; exit 3", Duration::from_secs(10)).unwrap();
        assert_eq!(result.exit_code, Some(3));
        assert_eq!(result.stdout.trim(), dir.display().to_string());
        assert_eq!(result.stderr.trim(), "aviso");
        assert!(!result.timed_out && !result.truncated);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// F9.10: saída grande guarda só o fim (64 KiB por stream).
    #[test]
    fn saida_grande_guarda_so_o_fim() {
        let dir = temp_dir("saida");
        let result = run_on_host(
            &dir,
            "for i in $(seq 1 5000); do echo linha-$i-com-texto-para-ocupar-espaco; done",
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(result.truncated);
        assert!(result.stdout.len() <= MAX_HOST_OUTPUT);
        assert!(
            result
                .stdout
                .trim_end()
                .ends_with("linha-5000-com-texto-para-ocupar-espaco")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// F9.10: no timeout, o grupo inteiro morre (inclusive o que ficou em segundo plano).
    #[test]
    fn timeout_mata_o_grupo_inteiro() {
        let dir = temp_dir("timeout");
        let marker = dir.join("sobreviveu");
        let script = format!("(sleep 3; touch {}) & sleep 30", marker.display());
        let result = run_on_host(&dir, &script, Duration::from_millis(300)).unwrap();
        assert!(result.timed_out);
        assert_eq!(result.exit_code, None);
        std::thread::sleep(Duration::from_secs(4));
        assert!(!marker.exists(), "o processo em segundo plano sobreviveu");
        std::fs::remove_dir_all(&dir).ok();
    }
}
