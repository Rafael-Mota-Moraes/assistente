//! Rotas HTTP do hubd. Só `/api/health` é aberta; o resto exige `Authorization: Bearer <token>`
//! (no WebSocket do terminal, que não manda header, vale `?token=`). A web UI do ai-memory
//! (`/web`, `/api/v1`, F5.5) aceita também o cookie que `/memoria/entrar?token=` entrega.
//! Rotas ainda não implementadas respondem 501 com o ID da funcionalidade (docs/FUNCIONALIDADES.md).
//! A API dos agentes (`agent_router`, F9.5) é outro roteador, servido só no `agent_listen`:
//! é a única parte do hubd que o ai-jail alcança, autenticada pelo token da sessão.

use axum::extract::{Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, delete, get, post};
use axum::{Json, Router};
use http_body_util::{BodyExt, Full};
use hub_core::{
    AgentKind, Config, ExecAccess, LaunchOptions, LaunchPlan, Project, ProjectSettings,
    SessionSpec, claude_logged_in, discover_projects, exec_client_dir, load_settings,
    settings_path,
};
use hyper::client::conn::http1;
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::exec::{self, ExecError, ExecJob, ExecStore, ExecView, NewRequest};
use crate::sessions::{SessionInfo, Tmux, new_exec_token, next_seq};
use crate::sleep::{
    self, CollectReport, Remote, SleepLimits, SleepStatusReport, StartSleepError, StartedSleep,
    request_stop,
};
use crate::terminal;
use crate::tunnel::{TunnelHandle, TunnelStatus};

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    config: Config,
    home: PathBuf,
    token: Option<String>,
    started: Instant,
    /// `None` com `--no-tunnel` (desenvolvimento fora da VM).
    tunnel: Option<TunnelHandle>,
    tmux: Tmux,
    /// Só nos testes: troca o comando do agente (ai-jail + claude) por outro.
    argv_override: Option<Vec<String>>,
    /// Só nos testes: troca o notebook (ssh) pelo host local.
    sleep_remote_override: Option<Remote>,
    /// Só nos testes: env extra do `dorme` (agente falso, sem jail) e o comando no lugar
    /// do `dorme` instalado (o script do repo).
    sleep_env_override: Vec<(String, String)>,
    dorme_override: Option<String>,
    /// Pasta com o build da UI servido em `/ui/*` (F8.3).
    ui_dir: Option<PathBuf>,
    /// Serializa leitura-modificação-escrita de `~/.assistente/projetos.toml` (F9.2).
    settings_lock: tokio::sync::Mutex<()>,
    /// Pedidos dos agentes para rodar fora do jail (F9.5).
    exec: ExecStore,
    /// Porta do `agent_listen` já aberta; `None` = sessões nascem sem API dos agentes.
    exec_port: Option<u16>,
    /// Valor do cookie da web UI do ai-memory (F5.5), sorteado a cada início do hubd.
    memory_session: String,
    /// Só nos testes: troca o comando do atualizador (F8.5).
    update_command_override: Option<String>,
    /// Só nos testes: env extra do atualizador.
    update_env_override: Vec<(String, String)>,
}

impl AppState {
    pub fn new(
        config: Config,
        home: PathBuf,
        token: Option<String>,
        tunnel: Option<TunnelHandle>,
        tmux: Tmux,
    ) -> Self {
        // Padrão da VM (F8.5 instala cada versão aqui); `with_ui_dir` sobrescreve.
        let ui_dir = home.join(".assistente/versoes/atual/ui");
        Self(Arc::new(Inner {
            config,
            home,
            token,
            started: Instant::now(),
            tunnel,
            tmux,
            argv_override: None,
            sleep_remote_override: None,
            sleep_env_override: Vec::new(),
            dorme_override: None,
            ui_dir: ui_dir.is_dir().then_some(ui_dir),
            settings_lock: tokio::sync::Mutex::new(()),
            exec: ExecStore::default(),
            exec_port: None,
            memory_session: new_exec_token().unwrap_or_default(),
            update_command_override: None,
            update_env_override: Vec::new(),
        }))
    }

    /// Liga a API dos agentes nas sessões novas (o `agent_listen` já está escutando).
    pub fn with_exec_port(self, port: Option<u16>) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.exec_port = port;
        Self(Arc::new(inner))
    }

    #[cfg(test)]
    fn with_exec_store(self, store: ExecStore) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.exec = store;
        Self(Arc::new(inner))
    }

    /// Serve a UI de `dir` (`None` desliga o `/ui/`). Antes de compartilhar o estado.
    pub fn with_ui_dir(self, dir: Option<PathBuf>) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.ui_dir = dir;
        Self(Arc::new(inner))
    }

    /// Só nos testes: troca o comando do atualizador (F8.5).
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn with_update_command(self, command: String, env: Vec<(String, String)>) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.update_command_override = Some(command);
        inner.update_env_override = env;
        Self(Arc::new(inner))
    }

    #[cfg(test)]
    fn with_argv_override(self, argv: Vec<String>) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.argv_override = Some(argv);
        Self(Arc::new(inner))
    }

    #[cfg(test)]
    fn with_sleep_remote(self, remote: Remote) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.sleep_remote_override = Some(remote);
        Self(Arc::new(inner))
    }

    #[cfg(test)]
    fn with_dorme(self, command: String, env: Vec<(String, String)>) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.dorme_override = Some(command);
        inner.sleep_env_override = env;
        Self(Arc::new(inner))
    }

    /// Como falar com o notebook: ssh pelo alias da config (produção) ou o host local
    /// (testes).
    fn sleep_remote(&self) -> Remote {
        self.0
            .sleep_remote_override
            .clone()
            .unwrap_or_else(|| Remote::Ssh {
                host: self.0.config.notebook.ssh_host.clone(),
            })
    }
}

pub fn router(state: AppState) -> Router {
    let ui_dir = state.0.ui_dir.clone();
    let protected = Router::new()
        .route("/api/profiles", get(profiles))
        .route("/api/projects", get(projects))
        .route(
            "/api/projects/{profile}/{project}/settings",
            get(project_settings).put(save_project_settings),
        )
        .route(
            "/api/projects/{profile}/{project}/files",
            get(list_project_files),
        )
        .route(
            "/api/projects/{profile}/{project}/files/{*path}",
            get(read_project_file).put(write_project_file),
        )
        .route(
            "/api/projects/{profile}/{project}/git/status",
            get(git_status),
        )
        .route(
            "/api/projects/{profile}/{project}/git/diff/{*path}",
            get(git_diff),
        )
        .route("/api/notebook", get(notebook))
        .route("/api/memory", get(memory))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", delete(delete_session))
        .route("/api/sessions/{id}/terminal", get(open_terminal))
        .route("/api/sleep", get(sleep_status).post(start_sleep))
        .route("/api/sleep/stop", post(stop_sleep))
        .route("/api/sleep/collect", post(collect_sleep))
        .route("/api/sleep/merge", post(merge_sleep))
        .route("/api/update", get(update_status))
        .route("/api/update/apply", post(apply_update))
        .route("/api/update/rollback", post(rollback_update))
        .route("/api/exec", get(list_exec))
        .route("/api/exec/{id}/approve", post(approve_exec))
        .route("/api/exec/{id}/deny", post(deny_exec))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    let mut public = Router::new()
        .route("/api/health", get(health))
        // Web UI do ai-memory local (F5.5): o app abre `/memoria/entrar?token=` numa janela,
        // ganha o cookie e o proxy encaminha `/web` e `/api/v1` com o token do ai-memory.
        .route("/memoria/entrar", get(memory_login))
        .route("/web", any(proxy_ai_memory))
        .route("/web/{*path}", any(proxy_ai_memory))
        .route("/api/v1", any(proxy_ai_memory))
        .route("/api/v1/{*path}", any(proxy_ai_memory));
    // O bundle da UI não tem segredo: `/ui/*` é aberto (spec 09, F8.3).
    if let Some(dir) = ui_dir {
        public = public.nest_service("/ui", ServeDir::new(dir));
    }

    public.merge(protected).layer(cors()).with_state(state)
}

/// API dos agentes (F9.5), servida só no `agent_listen` (loopback, encaminhado ao jail). Sem
/// CORS nem `/api/*`: quem fala aqui é o cliente `fora-do-jail`, com o token da sessão.
pub fn agent_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/exec", post(agent_submit))
        .route("/v1/exec/{id}", get(agent_result))
        .with_state(state)
}

/// O app Tauri chama o hubd de outra origem (tauri://localhost) e, em dev, do Vite.
fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin([
            HeaderValue::from_static("tauri://localhost"),
            HeaderValue::from_static("http://tauri.localhost"),
            HeaderValue::from_static("http://localhost:1420"),
        ])
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

async fn require_token(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Some(expected) = &state.0.token else {
        return next.run(request).await;
    };
    let from_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    // O WebSocket do navegador não manda header: só a rota do terminal aceita `?token=`.
    let from_query = || {
        request
            .uri()
            .path()
            .ends_with("/terminal")
            .then(|| request.uri().query())
            .flatten()
            .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token=")))
            .map(str::to_owned)
    };
    let given = from_header.or_else(from_query);
    match given {
        Some(given) if constant_time_eq(given.as_bytes(), expected.as_bytes()) => {
            next.run(request).await
        }
        _ => error(StatusCode::UNAUTHORIZED, "token ausente ou inválido"),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

#[derive(Serialize)]
struct Commits {
    /// Do binário do hubd (build.rs grava o HEAD do build); `"desconhecido"` fora do git.
    hubd: &'static str,
    /// Da UI servida: `commit.txt` ao lado do `index.html` (`just build-ui`). `null` sem UI.
    ui: Option<String>,
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    version: &'static str,
    uptime_seconds: u64,
    /// F8.7 compara com o commit que o app já tem para saber se precisa recarregar.
    commit: Commits,
    /// F8.6: projeto do próprio Assistente editado pela aba Assistente.
    self_update: Option<SelfUpdateView>,
}

#[derive(Serialize)]
struct SelfUpdateView {
    profile: String,
    project: String,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state.0.started.elapsed().as_secs(),
        commit: Commits {
            hubd: env!("HUBD_COMMIT"),
            ui: ui_commit(state.0.ui_dir.as_deref()),
        },
        self_update: state.0.config.self_update.as_ref().map(|s| SelfUpdateView {
            profile: s.profile.clone(),
            project: s.project.clone(),
        }),
    })
}

/// Conteúdo de `<ui_dir>/commit.txt`. `None` sem UI servida ou sem o arquivo.
fn ui_commit(ui_dir: Option<&std::path::Path>) -> Option<String> {
    let text = std::fs::read_to_string(ui_dir?.join("commit.txt")).ok()?;
    let commit = text.trim();
    (!commit.is_empty()).then(|| commit.to_owned())
}

#[derive(Serialize)]
struct ProfileView {
    id: String,
    label: String,
    color: String,
    root: PathBuf,
    /// `false` = o Claude deste perfil nunca fez login: a UI avisa em vez de abrir uma aba
    /// que morre na hora.
    claude_login: bool,
}

async fn profiles(State(state): State<AppState>) -> Json<Vec<ProfileView>> {
    Json(
        state
            .0
            .config
            .profiles
            .iter()
            .map(|(id, p)| ProfileView {
                id: id.clone(),
                label: p.label.clone(),
                color: p.color.clone(),
                root: p.root.clone(),
                claude_login: claude_logged_in(&state.0.config, &state.0.home, id),
            })
            .collect(),
    )
}

#[derive(Deserialize)]
struct ProjectsQuery {
    profile: String,
}

async fn projects(
    State(state): State<AppState>,
    Query(query): Query<ProjectsQuery>,
) -> Result<Json<Vec<Project>>, Response> {
    let Some(profile) = state.0.config.profile(&query.profile) else {
        return Err(error(StatusCode::NOT_FOUND, "perfil desconhecido"));
    };
    let root = profile.root.clone();
    tokio::task::spawn_blocking(move || discover_projects(&root))
        .await
        .map_err(|_| {
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "falha ao listar projetos",
            )
        })?
        .map(Json)
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))
}

#[derive(Serialize)]
struct ServiceStatus {
    name: String,
    port: u16,
    up: bool,
    /// Por que o serviço está caído (porta muda, HTTP inesperado…); `null` quando de pé.
    last_error: Option<String>,
    /// Segundos Unix da última verificação (tooltip da UI).
    checked_at: u64,
}

#[derive(Serialize)]
struct NotebookStatus {
    ssh_host: String,
    /// false com `hubd --no-tunnel`: aí só testamos as portas locais.
    tunnel_managed: bool,
    tunnel: Option<TunnelStatus>,
    services: Vec<ServiceStatus>,
}

/// GET mínimo por cima do tokio (sem crate extra): devolve o status HTTP ou o erro legível.
async fn http_probe(port: u16, path: &str) -> Result<u16, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::ConnectionRefused => format!("porta {port} não escuta"),
            _ => format!("porta {port}: {e}"),
        })?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: */*\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| format!("porta {port}: {e}"))?;
    let mut head = [0u8; 96];
    let n = stream
        .read(&mut head)
        .await
        .map_err(|e| format!("porta {port}: {e}"))?;
    if n == 0 {
        return Err(format!(
            "porta {port} fechou sem responder (túnel sem o serviço?)"
        ));
    }
    let text = String::from_utf8_lossy(&head[..n]);
    text.split(' ')
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("porta {port} não respondeu HTTP: {}", text.trim()))
}

/// Respondeu HTTP e parece o serviço certo (auth pedida conta como de pé).
fn probe_is_up(status: u16) -> bool {
    (200..=399).contains(&status) || status == 401 || status == 403
}

/// Um serviço do notebook está "de pé" se responde HTTP na porta encaminhada em 127.0.0.1.
/// Connect TCP dava falso positivo (qualquer processo na porta); os endpoints são os da
/// spec 07: `GET /v1/models` no 9router e `GET /` no ai-memory.
async fn notebook(State(state): State<AppState>) -> Json<NotebookStatus> {
    let notebook = &state.0.config.notebook;
    let mut services = Vec::new();
    for (name, &port) in &notebook.tunnel {
        if name == "ai_memory" {
            continue;
        }
        let path = match name.as_str() {
            "9router" => "/v1/models",
            _ => "/",
        };
        let (up, last_error) =
            match tokio::time::timeout(Duration::from_secs(1), http_probe(port, path)).await {
                Err(_) => (false, Some(format!("porta {port}: sem resposta em 1 s"))),
                Ok(Ok(status)) if probe_is_up(status) => (true, None),
                Ok(Ok(status)) => (false, Some(format!("HTTP {status} em {path}"))),
                Ok(Err(e)) => (false, Some(e)),
            };
        let checked_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        services.push(ServiceStatus {
            name: name.clone(),
            port,
            up,
            last_error,
            checked_at,
        });
    }
    Json(NotebookStatus {
        ssh_host: notebook.ssh_host.clone(),
        tunnel_managed: state.0.tunnel.is_some(),
        tunnel: state.0.tunnel.as_ref().map(TunnelHandle::status),
        services,
    })
}

#[derive(Serialize)]
struct MemoryStatus {
    port: u16,
    data_dir: String,
    mirror_dir: String,
    up: bool,
    last_error: Option<String>,
    checked_at: u64,
    server_version: Option<String>,
    last_commit: Option<String>,
    web_url: String,
}

/// Servidor ai-memory local na VM (F5.0), independente do notebook.
async fn memory(State(state): State<AppState>) -> Json<MemoryStatus> {
    let memory = &state.0.config.memory;
    let port = memory.port;
    let (up, last_error) =
        match tokio::time::timeout(Duration::from_secs(1), http_probe(port, "/healthz")).await {
            Err(_) => (false, Some(format!("porta {port}: sem resposta em 1 s"))),
            Ok(Ok(status)) if probe_is_up(status) => (true, None),
            Ok(Ok(status)) => (false, Some(format!("HTTP {status} em /healthz"))),
            Ok(Err(e)) => (false, Some(e)),
        };
    let checked_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let server_version = read_ai_memory_version();
    let last_commit = read_wiki_last_commit(&memory.mirror_dir);
    Json(MemoryStatus {
        port,
        data_dir: memory.data_dir.display().to_string(),
        mirror_dir: memory.mirror_dir.display().to_string(),
        up,
        last_error,
        checked_at,
        server_version,
        last_commit,
        web_url: "/web".to_string(),
    })
}

/// Versão do binário `ai-memory` no PATH (F5.6). `None` se ausente ou ilegível.
fn read_ai_memory_version() -> Option<String> {
    let output = std::process::Command::new("ai-memory")
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim();
    if first.is_empty() {
        None
    } else {
        Some(first.to_string())
    }
}

/// Último commit do wiki git em `mirror_dir` (ou `mirror_dir/wiki`) (F5.6).
fn read_wiki_last_commit(mirror_dir: &std::path::Path) -> Option<String> {
    let wiki = sleep::wiki_repo(mirror_dir);
    if !wiki.join(".git").exists() {
        return None;
    }
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &wiki.display().to_string(),
            "log",
            "-1",
            "--format=%H %s",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Cookie da web UI do ai-memory (F5.5). O valor é `Inner::memory_session`, não o token do hub.
const MEMORY_COOKIE: &str = "hub_memoria";

#[derive(Deserialize)]
struct MemoryLogin {
    token: Option<String>,
}

/// Porta de entrada da web UI do ai-memory (F5.5): troca o token do hub (na query, porque a
/// janela do app só abre uma URL) pelo cookie da sessão e redireciona para `/web`.
async fn memory_login(State(state): State<AppState>, Query(login): Query<MemoryLogin>) -> Response {
    let mut response = (StatusCode::SEE_OTHER, [(header::LOCATION, "/web")]).into_response();
    let Some(expected) = &state.0.token else {
        return response;
    };
    let valid = login
        .token
        .is_some_and(|given| constant_time_eq(given.as_bytes(), expected.as_bytes()));
    if !valid || state.0.memory_session.is_empty() {
        return error(StatusCode::UNAUTHORIZED, "token ausente ou inválido");
    }
    let cookie = format!(
        "{MEMORY_COOKIE}={}; Path=/; HttpOnly; SameSite=Strict",
        state.0.memory_session
    );
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

/// Quem chega ao proxy: o app com o token do hub ou a janela com o cookie de `/memoria/entrar`.
fn memory_access_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = &state.0.token else {
        return true;
    };
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|given| constant_time_eq(given.as_bytes(), expected.as_bytes()));
    let session = &state.0.memory_session;
    let cookie = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().strip_prefix(&format!("{MEMORY_COOKIE}=")))
        .any(|given| !session.is_empty() && constant_time_eq(given.as_bytes(), session.as_bytes()));
    bearer || cookie
}

/// Token do servidor ai-memory da VM, criado pelo `ai-memory init` (vm/provisionar.sh).
fn ai_memory_token(state: &AppState) -> Option<String> {
    let path = state.0.config.memory.data_dir.join("auth-tokens/default");
    std::fs::read_to_string(path)
        .ok()
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

/// Proxy reverso para o servidor ai-memory local na VM (F5.5). Encaminha a requisição
/// (método, cabeçalhos, corpo, query) ao `127.0.0.1:{port}` trocando o `Authorization` pelo
/// token do ai-memory, então a web UI abre sem pedir login.
async fn proxy_ai_memory(State(state): State<AppState>, req: Request) -> Response {
    if !memory_access_allowed(&state, req.headers()) {
        return error(
            StatusCode::UNAUTHORIZED,
            "abra a memória pelo app (botão Memória na lateral)",
        );
    }
    let port = state.0.config.memory.port;
    // Origin-form (`/web?x=1`): o endereço já está no `Host`, não na linha do pedido.
    let target = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned());

    let stream = match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
        Ok(s) => s,
        Err(e) => {
            return error(
                StatusCode::BAD_GATEWAY,
                &format!("ai-memory inacessível: {e}"),
            );
        }
    };
    let io = TokioIo::new(stream);
    let (mut sender, conn) = match http1::handshake(io).await {
        Ok(x) => x,
        Err(e) => {
            return error(
                StatusCode::BAD_GATEWAY,
                &format!("handshake ai-memory: {e}"),
            );
        }
    };
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let method = req.method().clone();
    let mut builder = Request::builder().method(method).uri(&target);
    for (k, v) in req.headers() {
        if k == header::HOST || k == header::AUTHORIZATION {
            continue;
        }
        builder = builder.header(k, v);
    }
    builder = builder.header(header::HOST, format!("127.0.0.1:{}", port));
    if let Some(token) = ai_memory_token(&state) {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }

    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => return error(StatusCode::BAD_REQUEST, &format!("corpo: {e}")),
    };

    let upstream_req = match builder.body(Full::new(body_bytes)) {
        Ok(r) => r,
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, &format!("request: {e}")),
    };

    let upstream_res = match sender.send_request(upstream_req).await {
        Ok(r) => r,
        Err(e) => return error(StatusCode::BAD_GATEWAY, &format!("ai-memory: {e}")),
    };

    let status = upstream_res.status();
    let headers = upstream_res.headers().clone();
    let body = upstream_res.into_body();
    let bytes = match body.collect().await {
        Ok(c) => c.to_bytes(),
        Err(e) => return error(StatusCode::BAD_GATEWAY, &format!("corpo ai-memory: {e}")),
    };

    let mut res = Response::builder().status(status);
    for (k, v) in headers.iter() {
        res = res.header(k, v);
    }
    res.body(axum::body::Body::from(bytes))
        .unwrap_or_else(|_| error(StatusCode::INTERNAL_SERVER_ERROR, "resposta inválida"))
}

fn internal(message: &str) -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, message)
}

/// POST /api/sleep/stop (F6.4): cria o arquivo STOP no notebook. O `dorme` confere no topo
/// de cada iteração: a tarefa em curso termina e o loop para. Sem nada rodando, é inofensivo
/// (o `dorme` apaga o STOP ao iniciar).
async fn stop_sleep(State(state): State<AppState>) -> Result<StatusCode, Response> {
    let stop_file = state
        .0
        .config
        .notebook
        .workdir
        .join("STOP")
        .display()
        .to_string();
    request_stop(&state.sleep_remote(), &stop_file)
        .await
        .map_err(|e| {
            error(
                StatusCode::BAD_GATEWAY,
                &format!("o STOP não chegou ao notebook: {e}"),
            )
        })?;
    tracing::info!("parada do modo dormir pedida: {stop_file}");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SleepRequest {
    profile: String,
    project: String,
    /// `YYYY-MM-DD` da noite de sono (a mesma usada ao iniciar, F6.3).
    date: String,
}

/// `<raiz do perfil>/<projeto>` no PC (montado na VM no mesmo caminho), com as mesmas
/// garantias do create_session: nome de projeto sem caminho e `.git` presente.
/// O `Err` é pequeno de propósito (clippy::result_large_err): vira `Response` no chamador.
fn sleep_project_dir(
    state: &AppState,
    profile: &str,
    project: &str,
) -> Result<PathBuf, (StatusCode, &'static str)> {
    let Some(profile_config) = state.0.config.profile(profile) else {
        return Err((StatusCode::NOT_FOUND, "perfil desconhecido"));
    };
    if project.is_empty() || project.starts_with('.') || project.contains(['/', '\\']) {
        return Err((StatusCode::BAD_REQUEST, "nome de projeto inválido"));
    }
    let dir = profile_config.root.join(project);
    if !dir.join(".git").exists() || !dir.starts_with(&profile_config.root) {
        return Err((StatusCode::NOT_FOUND, "projeto não encontrado neste perfil"));
    }
    Ok(dir)
}

/// `<workdir>/projetos/<perfil>/<projeto>`: onde o sync (F6.2) cria o clone no notebook.
fn sleep_remote_dir(state: &AppState, profile: &str, project: &str) -> String {
    state
        .0
        .config
        .notebook
        .workdir
        .join("projetos")
        .join(profile)
        .join(project)
        .display()
        .to_string()
}

#[derive(Deserialize)]
struct SleepStartRequest {
    profile: String,
    project: String,
    /// Árvore suja: autoriza o commit WIP antes de dormir (a UI oferece).
    #[serde(default)]
    wip: bool,
}

#[derive(Deserialize)]
struct SleepQuery {
    profile: String,
    project: String,
}

/// GET /api/sleep (F6.3): o `dorme` está rodando no notebook? + tail do log ao vivo.
async fn sleep_status(
    State(state): State<AppState>,
    Query(query): Query<SleepQuery>,
) -> Result<Json<SleepStatusReport>, Response> {
    sleep_project_dir(&state, &query.profile, &query.project)
        .map_err(|(status, msg)| error(status, msg))?;
    let workdir = state.0.config.notebook.workdir.display().to_string();
    sleep::sleep_status(&state.sleep_remote(), &workdir, &query.project)
        .await
        .map(Json)
        .map_err(|e| {
            error(
                StatusCode::BAD_GATEWAY,
                &format!("não consegui ver o notebook: {e}"),
            )
        })
}

/// POST /api/sleep (F6.3): entrega o projeto ao notebook — árvore limpa (ou WIP), sync,
/// briefing do ai-memory, prompt e `dorme` num tmux de lá. O PC pode desligar depois.
async fn start_sleep(
    State(state): State<AppState>,
    Json(request): Json<SleepStartRequest>,
) -> Result<(StatusCode, Json<StartedSleep>), Response> {
    let inner = &state.0;
    let dir = sleep_project_dir(&state, &request.profile, &request.project)
        .map_err(|(status, msg)| error(status, msg))?;
    let profile = inner
        .config
        .profile(&request.profile)
        .expect("sleep_project_dir já validou o perfil");

    // 1. Árvore limpa (ou commit WIP autorizado no pedido).
    sleep::ensure_clean_tree(&dir, request.wip)
        .await
        .map_err(|e| error(StatusCode::CONFLICT, &e.to_string()))?;

    // 2. Limites do dormir vêm do projeto (.assistente.toml; ausente = padrões).
    let text = std::fs::read_to_string(dir.join(".assistente.toml")).unwrap_or_default();
    let project_config = hub_core::sleep::ProjectConfig::parse(&text).map_err(|e| {
        error(
            StatusCode::BAD_REQUEST,
            &format!(".assistente.toml inválido: {e}"),
        )
    })?;

    // 3. A data de hoje nomeia a branch dorme/<data>.
    let date = sleep::today().await.map_err(|e| internal(&e.to_string()))?;

    // 4. Briefing do ai-memory local (F5.0), melhor esforço: sem ele o prompt segue sem
    //    a seção de memória (o agente noturno consulta a réplica do notebook, se houver).
    let token = std::fs::read_to_string(inner.config.memory.data_dir.join("auth-tokens/default"))
        .ok()
        .map(|token| token.trim().to_owned());
    let briefing = sleep::export_briefing(
        inner.config.memory.port,
        token.as_deref(),
        &profile.memory_workspace,
        &request.project,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("dormindo sem briefing: {e}");
        String::new()
    });

    // 5. Prompt da iteração (template do hub-core).
    let prompt =
        hub_core::sleep::render_prompt(&project_config.sleep, &format!("dorme/{date}"), &briefing);

    // 5b. Sync da memória (wiki) para o notebook (F5.4), best-effort: o agente noturno
    //     consulta a réplica local; falha não trava o dormir.
    let memory_remote_dir = inner
        .config
        .notebook
        .workdir
        .join("memoria")
        .display()
        .to_string();
    if let Err(e) = sleep::sync_memory(
        &inner.config.memory.mirror_dir,
        &state.sleep_remote(),
        &memory_remote_dir,
    )
    .await
    {
        tracing::warn!("dormindo sem sync da memória: {e}");
    }

    // 6. Sync (F6.2) + prompt em briefings/ + dorme num tmux do notebook.
    let workdir = inner.config.notebook.workdir.display().to_string();
    let remote_dir = sleep_remote_dir(&state, &request.profile, &request.project);
    let started = sleep::start_sleep(sleep::StartSleep {
        project_dir: &dir,
        remote: &state.sleep_remote(),
        remote_dir: &remote_dir,
        workdir: &workdir,
        project: &request.project,
        date: &date,
        prompt: &prompt,
        limits: SleepLimits {
            max_minutes: project_config.sleep.max_minutes,
            attempts: project_config.sleep.attempts,
            model: project_config.sleep.model.clone(),
        },
        dorme_command: inner.dorme_override.as_deref().unwrap_or("dorme"),
        extra_env: &inner.sleep_env_override,
    })
    .await
    .map_err(|e| match e {
        StartSleepError::AlreadyRunning => error(
            StatusCode::CONFLICT,
            "o dorme deste projeto já está rodando",
        ),
        StartSleepError::Other(e) => error(
            StatusCode::BAD_GATEWAY,
            &format!("não consegui iniciar o modo dormir no notebook: {e}"),
        ),
    })?;
    tracing::info!(
        "modo dormir iniciado: {} (noite de {date})",
        started.session
    );
    Ok((StatusCode::ACCEPTED, Json(started)))
}

/// POST /api/sleep/collect (F6.5): busca `dorme/<data>` e `agente/*` do notebook para
/// `refs/remotes/notebook/*` e devolve o resumo da manhã.
async fn collect_sleep(
    State(state): State<AppState>,
    Json(request): Json<SleepRequest>,
) -> Result<Json<CollectReport>, Response> {
    let dir = sleep_project_dir(&state, &request.profile, &request.project)
        .map_err(|(status, msg)| error(status, msg))?;
    let remote_dir = sleep_remote_dir(&state, &request.profile, &request.project);
    let report = sleep::collect_project(&dir, &state.sleep_remote(), &remote_dir, &request.date)
        .await
        .map_err(|e| {
            error(
                StatusCode::BAD_GATEWAY,
                &format!("não consegui recolher do notebook: {e}"),
            )
        })?;
    // Recolhe também o wiki (memória) do notebook (F5.4), best-effort.
    let memory_remote_dir = state
        .0
        .config
        .notebook
        .workdir
        .join("memoria")
        .display()
        .to_string();
    if let Err(e) = sleep::collect_memory(
        &state.0.config.memory.mirror_dir,
        &state.sleep_remote(),
        &memory_remote_dir,
    )
    .await
    {
        tracing::warn!("recolhendo sem sync da memória: {e}");
    }
    Ok(Json(report))
}

/// POST /api/sleep/merge (F6.5): fast-forward da base até a branch recolhida. 409 quando
/// a base andou durante a noite (o merge é à mão; nada muda).
async fn merge_sleep(
    State(state): State<AppState>,
    Json(request): Json<SleepRequest>,
) -> Result<StatusCode, Response> {
    let dir = sleep_project_dir(&state, &request.profile, &request.project)
        .map_err(|(status, msg)| error(status, msg))?;
    sleep::merge_sleep_branch(&dir, &request.date)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|e| error(StatusCode::CONFLICT, &format!("merge recusado: {e}")))
}

// ---- Atualizador (F8.5) ----

#[derive(Deserialize)]
struct ApplyUpdateRequest {
    commit: String,
}

#[derive(Serialize, Clone)]
struct UpdateCommitView {
    hash: String,
    subject: String,
}

#[derive(Serialize)]
struct UpdateStatusView {
    running: bool,
    status: String,
    commit: Option<String>,
    current_ui: Option<String>,
    previous_commit: Option<String>,
    error: Option<String>,
    log_tail: Option<String>,
    log: Option<String>,
    head: Option<String>,
    commits: Vec<UpdateCommitView>,
    pending_commits: Vec<UpdateCommitView>,
    head_commit: Option<String>,
}

fn read_update_state(
    home: &std::path::Path,
) -> (bool, String, Option<String>, Option<String>, Option<String>) {
    // (running, status, commit, error, log_tail)
    let base = home.join(".assistente/atualizacoes");
    let mut running = false;
    let mut status = "idle".to_string();
    let mut commit = None;
    let mut error = None;
    let mut log_tail = None;
    let mut latest_mtime = std::time::SystemTime::UNIX_EPOCH;
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            let path = entry.path();
            let estado = path.join("estado.json");
            if !estado.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&estado).unwrap_or_default();
            let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            let st = parsed
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("idle")
                .to_string();
            let mt = std::fs::metadata(&estado)
                .and_then(|m| m.modified())
                .unwrap_or(latest_mtime);
            if mt >= latest_mtime {
                latest_mtime = mt;
                status = st.clone();
                commit = parsed
                    .get("commit")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                error = parsed
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                let log = path.join("log");
                if log.is_file() {
                    let text = std::fs::read_to_string(&log).unwrap_or_default();
                    let tail: String = text
                        .chars()
                        .rev()
                        .take(2000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    log_tail = Some(tail);
                }
            }
            if st == "running" {
                running = true;
            }
        }
    }
    if status == "running" {
        running = true;
    }
    (running, status, commit, error, log_tail)
}

async fn update_status(State(state): State<AppState>) -> Json<UpdateStatusView> {
    let (running, status, commit, error, log_tail) = read_update_state(&state.0.home);
    let versions = state.0.home.join(".assistente/versoes");
    let previous_commit = std::fs::read_link(versions.join("anterior"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .or_else(|| {
            let dir = versions.join("anterior");
            dir.read_dir().ok()?;
            dir.file_name().map(|n| n.to_string_lossy().to_string())
        });
    // Se `anterior` for diretório (não symlink), tenta ler commit.txt
    let previous_commit = previous_commit.or_else(|| {
        let p = versions.join("anterior/commit.txt");
        std::fs::read_to_string(p).ok().map(|s| s.trim().to_owned())
    });
    let current_ui = ui_commit(state.0.ui_dir.as_deref());

    // F8.6: commits recentes do projeto do próprio Assistente, para o painel mostrar só o que
    // ainda não foi aplicado. Versões já instaladas (e a que está rodando) ficam de fora.
    let applied: Vec<String> = std::fs::read_dir(&versions)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let path = entry.path();
                    (path.is_dir() && !path.is_symlink() && name != "atual" && name != "anterior")
                        .then_some(name)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut applied = applied;
    // O histórico de atualizações fica em `atualizacoes/<commit>/estado.json`; versões antigas
    // podem ter sido podadas de `versoes/`, mas continuam "finalizadas" e não devem voltar à lista.
    if let Ok(entries) = std::fs::read_dir(state.0.home.join(".assistente/atualizacoes")) {
        for entry in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path().join("estado.json")) else {
                continue;
            };
            let Ok(estado) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if estado.get("status").and_then(|v| v.as_str()) == Some("success")
                && let Some(done) = estado.get("commit").and_then(|v| v.as_str())
            {
                applied.push(done.to_owned());
            }
        }
    }
    if status == "success"
        && let Some(done) = &commit
    {
        applied.push(done.clone());
    }
    if let Some(ui) = &current_ui {
        applied.push(ui.clone());
    }
    applied.push(env!("HUBD_COMMIT").to_owned());

    let (head, commits) = match &state.0.config.self_update {
        Some(self_update) => {
            match sleep_project_dir(&state, &self_update.profile, &self_update.project) {
                Ok(dir) => tokio::task::spawn_blocking(move || {
                    let log = |args: &[&str]| -> Option<String> {
                        let output = std::process::Command::new("git")
                            .arg("-C")
                            .arg(&dir)
                            .args(args)
                            .output()
                            .ok()?;
                        output
                            .status
                            .success()
                            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                            .filter(|s| !s.is_empty())
                    };
                    let head = log(&["rev-parse", "--short", "HEAD"]);
                    let already_applied = |hash: &str| {
                        applied
                            .iter()
                            .any(|done| done.starts_with(hash) || hash.starts_with(done))
                    };
                    let commits = log(&["log", "-50", "--format=%h%x1f%s"])
                        .map(|text| {
                            text.lines()
                                .filter_map(|line| {
                                    let (hash, subject) = line.split_once('\u{1f}')?;
                                    (!already_applied(hash)).then(|| UpdateCommitView {
                                        hash: hash.to_owned(),
                                        subject: subject.to_owned(),
                                    })
                                })
                                .take(8)
                                .collect()
                        })
                        .unwrap_or_default();
                    (head, commits)
                })
                .await
                .unwrap_or((None, Vec::new())),
                Err(_) => (None, Vec::new()),
            }
        }
        None => (None, Vec::new()),
    };

    Json(UpdateStatusView {
        running,
        status,
        commit,
        current_ui,
        previous_commit,
        error,
        log: log_tail.clone(),
        log_tail,
        head: head.clone(),
        commits: commits.clone(),
        pending_commits: commits,
        head_commit: head,
    })
}

async fn apply_update(
    State(state): State<AppState>,
    Json(request): Json<ApplyUpdateRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), Response> {
    let commit = request.commit.trim().to_owned();
    if commit.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "commit obrigatório"));
    }
    // O commit vai para o atualizador: só hexadecimal, para não abrir injeção de shell.
    if !(7..=40).contains(&commit.len()) || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(error(StatusCode::BAD_REQUEST, "commit inválido"));
    }
    let mut command = match state.0.update_command_override.clone() {
        Some(cmd) => {
            let mut command = tokio::process::Command::new("setsid");
            command
                .arg("bash")
                .arg("-c")
                .arg(format!("{cmd} \"$1\""))
                .arg("bash")
                .arg(&commit);
            command
        }
        None => {
            let script = state
                .0
                .home
                .join(".local/share/assistente/repo/vm/atualizar.sh");
            if !script.is_file() {
                return Err(error(
                    StatusCode::CONFLICT,
                    "atualizador não instalado na VM (vm/atualizar.sh)",
                ));
            }
            let mut command = tokio::process::Command::new("setsid");
            command.arg("bash").arg(script).arg(&commit);
            command
        }
    };
    for (k, v) in &state.0.update_env_override {
        command.env(k, v);
    }
    command.current_dir(&state.0.home);
    if let Err(e) = command.spawn() {
        return Err(internal(&format!(
            "não consegui iniciar o atualizador: {e}"
        )));
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "commit": commit, "started": true })),
    ))
}

async fn rollback_update(State(state): State<AppState>) -> Result<StatusCode, Response> {
    let versions = state.0.home.join(".assistente/versoes");
    let atual = versions.join("atual");
    let anterior = versions.join("anterior");
    if !anterior.exists() {
        return Err(error(StatusCode::CONFLICT, "sem versão anterior"));
    }
    let tmp = versions.join("atual.rollback-tmp");
    if atual.exists() || atual.is_symlink() {
        let _ = std::fs::rename(&atual, &tmp);
    }
    std::fs::rename(&anterior, &atual).map_err(|e| internal(&e.to_string()))?;
    if tmp.exists() || tmp.is_symlink() {
        let _ = std::fs::rename(&tmp, &anterior);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct ProjectSettingsView {
    settings: ProjectSettings,
    /// `[jail] docker = true` no `.assistente.toml` do repo: liga o Docker mesmo com
    /// `settings.docker` desligado (compatibilidade; o agente pode editar esse arquivo).
    docker_from_project_file: bool,
}

fn settings_view(dir: &std::path::Path, settings: ProjectSettings) -> ProjectSettingsView {
    ProjectSettingsView {
        settings,
        docker_from_project_file: hub_core::sleep::jail_config_at(dir).docker,
    }
}

/// GET /api/projects/{profile}/{project}/settings (F9.2).
async fn project_settings(
    State(state): State<AppState>,
    Path((profile, project)): Path<(String, String)>,
) -> Result<Json<ProjectSettingsView>, Response> {
    let dir = sleep_project_dir(&state, &profile, &project).map_err(|(s, m)| error(s, m))?;
    let file = load_settings(&state.0.home).map_err(|e| internal(&e))?;
    Ok(Json(settings_view(&dir, file.get(&profile, &project))))
}

/// PUT /api/projects/{profile}/{project}/settings (F9.2): troca as configurações do projeto.
/// Valem para as sessões novas.
async fn save_project_settings(
    State(state): State<AppState>,
    Path((profile, project)): Path<(String, String)>,
    Json(settings): Json<ProjectSettings>,
) -> Result<Json<ProjectSettingsView>, Response> {
    let dir = sleep_project_dir(&state, &profile, &project).map_err(|(s, m)| error(s, m))?;
    settings
        .validate()
        .map_err(|e| error(StatusCode::BAD_REQUEST, &e))?;
    update_settings(&state, |file| {
        file.set(&profile, &project, settings.clone())
    })
    .await
    .map_err(|e| internal(&e))?;
    tracing::info!("configurações de {profile}/{project}: {settings:?}");
    Ok(Json(settings_view(&dir, settings)))
}

/// Lê, muda e grava `projetos.toml` sob o lock; escrita atômica (tmp + rename), modo 0600.
async fn update_settings(
    state: &AppState,
    change: impl FnOnce(&mut hub_core::SettingsFile),
) -> Result<(), String> {
    let _guard = state.0.settings_lock.lock().await;
    let mut file = load_settings(&state.0.home)?;
    change(&mut file);
    let path = settings_path(&state.0.home);
    let tmp = path.with_extension("toml.tmp");
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        out.write_all(file.to_toml().as_bytes())?;
        out.sync_all()?;
        std::fs::rename(&tmp, &path)
    };
    write().map_err(|e| format!("{}: {e}", path.display()))
}

// ---- Arquivos e git do projeto (F10.1) ----

const MAX_FILE_BYTES: usize = 1024 * 1024;

#[derive(Serialize)]
struct FileList {
    files: Vec<String>,
}

#[derive(Serialize)]
struct FileContent {
    path: String,
    content: String,
}

#[derive(Serialize)]
struct GitStatusEntry {
    path: String,
    status: String,
    staged: bool,
}

#[derive(Serialize)]
struct GitStatusView {
    files: Vec<GitStatusEntry>,
}

#[derive(Serialize)]
struct GitDiffView {
    path: String,
    head: Option<String>,
    working: Option<String>,
}

#[derive(Deserialize)]
struct WriteFileRequest {
    content: String,
}

/// Resolve um caminho relativo dentro do projeto, recusando `..`, absoluto e
/// symlinks que escapam da raiz.
fn resolve_project_file(
    project_dir: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, String> {
    if rel.is_empty() {
        return Err("caminho vazio".into());
    }
    let rel_path = std::path::Path::new(rel);
    if rel_path.is_absolute() {
        return Err("caminho absoluto não permitido".into());
    }
    for comp in rel_path.components() {
        match comp {
            std::path::Component::ParentDir => return Err("caminho com '..' não permitido".into()),
            std::path::Component::CurDir => {}
            std::path::Component::Normal(_) => {}
            _ => return Err("componente de caminho inválido".into()),
        }
    }
    let target = project_dir.join(rel_path);
    if target.exists() {
        let canonical = target
            .canonicalize()
            .map_err(|e| format!("não consegui resolver o caminho: {e}"))?;
        let project_canonical = project_dir
            .canonicalize()
            .map_err(|e| format!("não consegui resolver o projeto: {e}"))?;
        if !canonical.starts_with(&project_canonical) {
            return Err("caminho fora do projeto".into());
        }
        return Ok(canonical);
    }
    let mut ancestor = target.clone();
    while !ancestor.exists() {
        match ancestor.parent() {
            Some(p) => ancestor = p.to_path_buf(),
            None => break,
        }
    }
    if ancestor.exists() {
        let canonical = ancestor
            .canonicalize()
            .map_err(|e| format!("não consegui resolver o caminho: {e}"))?;
        let project_canonical = project_dir
            .canonicalize()
            .map_err(|e| format!("não consegui resolver o projeto: {e}"))?;
        if !canonical.starts_with(&project_canonical) {
            return Err("caminho fora do projeto".into());
        }
    }
    Ok(target)
}

fn is_binary(bytes: &[u8]) -> bool {
    if bytes.len() > MAX_FILE_BYTES {
        return true;
    }
    if bytes.contains(&0) {
        return true;
    }
    std::str::from_utf8(bytes).is_err()
}

async fn list_project_files(
    State(state): State<AppState>,
    Path((profile, project)): Path<(String, String)>,
) -> Result<Json<FileList>, Response> {
    let dir = sleep_project_dir(&state, &profile, &project).map_err(|(s, m)| error(s, m))?;
    let files = tokio::task::spawn_blocking(move || -> Result<Vec<String>, String> {
        let output = std::process::Command::new("git")
            .args([
                "-C",
                &dir.display().to_string(),
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
            ])
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let mut files: Vec<String> = output
            .stdout
            .split(|b| *b == 0)
            .filter(|chunk| !chunk.is_empty())
            .filter_map(|chunk| String::from_utf8(chunk.to_vec()).ok())
            .collect();
        files.sort();
        files.dedup();
        Ok(files)
    })
    .await
    .map_err(|_| internal("falha ao listar arquivos"))?
    .map_err(|e| internal(&e))?;
    Ok(Json(FileList { files }))
}

async fn read_project_file(
    State(state): State<AppState>,
    Path((profile, project, path)): Path<(String, String, String)>,
) -> Result<Json<FileContent>, Response> {
    let dir = sleep_project_dir(&state, &profile, &project).map_err(|(s, m)| error(s, m))?;
    let rel = path;
    let target =
        resolve_project_file(&dir, &rel).map_err(|e| error(StatusCode::BAD_REQUEST, &e))?;
    let body = tokio::task::spawn_blocking(move || -> Result<String, (StatusCode, String)> {
        let meta =
            std::fs::metadata(&target).map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
        if meta.len() as usize > MAX_FILE_BYTES {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("arquivo maior que {} bytes", MAX_FILE_BYTES),
            ));
        }
        let bytes = std::fs::read(&target)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if is_binary(&bytes) {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "arquivo binário ou não UTF-8".into(),
            ));
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "arquivo não UTF-8".into(),
            )
        })?;
        Ok(text)
    })
    .await
    .map_err(|_| internal("falha ao ler arquivo"))?
    .map_err(|(s, m)| error(s, &m))?;
    Ok(Json(FileContent {
        path: rel,
        content: body,
    }))
}

async fn write_project_file(
    State(state): State<AppState>,
    Path((profile, project, path)): Path<(String, String, String)>,
    Json(request): Json<WriteFileRequest>,
) -> Result<StatusCode, Response> {
    let dir = sleep_project_dir(&state, &profile, &project).map_err(|(s, m)| error(s, m))?;
    if request.content.len() > MAX_FILE_BYTES {
        return Err(error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "conteúdo maior que o limite",
        ));
    }
    let rel = path;
    let target =
        resolve_project_file(&dir, &rel).map_err(|e| error(StatusCode::BAD_REQUEST, &e))?;
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = target.with_extension("tmp");
        std::fs::write(&tmp, request.content.as_bytes()).map_err(|e| e.to_string())?;
        if let Ok(f) = std::fs::File::open(&tmp) {
            let _ = f.sync_all();
        }
        std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|_| internal("falha ao gravar arquivo"))?
    .map_err(|e| internal(&e))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn git_status(
    State(state): State<AppState>,
    Path((profile, project)): Path<(String, String)>,
) -> Result<Json<GitStatusView>, Response> {
    let dir = sleep_project_dir(&state, &profile, &project).map_err(|(s, m)| error(s, m))?;
    let entries = tokio::task::spawn_blocking(move || -> Result<Vec<GitStatusEntry>, String> {
        let output = std::process::Command::new("git")
            .args(["-C", &dir.display().to_string(), "status", "--porcelain=v1"])
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut out = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if line.len() < 4 {
                continue;
            }
            let x = line.chars().next().unwrap_or(' ');
            let y = line.chars().nth(1).unwrap_or(' ');
            let rest = &line[3..];
            let path = if let Some(idx) = rest.find(" -> ") {
                rest[idx + 4..].to_string()
            } else {
                rest.to_string()
            };
            let (status, staged) = classify_status(x, y);
            out.push(GitStatusEntry {
                path,
                status,
                staged,
            });
        }
        Ok(out)
    })
    .await
    .map_err(|_| internal("falha ao consultar git status"))?
    .map_err(|e| internal(&e))?;
    Ok(Json(GitStatusView { files: entries }))
}

fn classify_status(x: char, y: char) -> (String, bool) {
    if x == '?' {
        return ("untracked".into(), false);
    }
    let staged_status = match x {
        'A' => Some("added"),
        'M' => Some("modified"),
        'D' => Some("deleted"),
        'R' => Some("renamed"),
        'C' => Some("copied"),
        _ => None,
    };
    if let Some(s) = staged_status {
        return (s.into(), true);
    }
    match y {
        'M' => ("modified".into(), false),
        'D' => ("deleted".into(), false),
        '?' => ("untracked".into(), false),
        'A' => ("added".into(), false),
        _ => ("modified".into(), false),
    }
}

#[derive(Deserialize, Default)]
struct DiffQuery {
    from: Option<String>,
    to: Option<String>,
}

async fn git_diff(
    State(state): State<AppState>,
    Path((profile, project, path)): Path<(String, String, String)>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<GitDiffView>, Response> {
    let dir = sleep_project_dir(&state, &profile, &project).map_err(|(s, m)| error(s, m))?;
    let rel = path;
    resolve_project_file(&dir, &rel).map_err(|e| error(StatusCode::BAD_REQUEST, &e))?;
    let from = query.from.unwrap_or_else(|| "HEAD".to_string());
    let to = query.to;
    let dir_clone = dir.clone();
    let rel_clone = rel.clone();
    let (head, working) =
        tokio::task::spawn_blocking(move || -> (Option<String>, Option<String>) {
            let show = |refname: &str| -> Option<String> {
                std::process::Command::new("git")
                    .args([
                        "-C",
                        &dir_clone.display().to_string(),
                        "show",
                        &format!("{refname}:{rel_clone}"),
                    ])
                    .output()
                    .ok()
                    .and_then(|o| {
                        if !o.status.success() {
                            return None;
                        }
                        String::from_utf8(o.stdout).ok()
                    })
            };
            let head = show(&from);
            let working = match to {
                Some(refname) => show(&refname),
                None => {
                    let target = dir_clone.join(&rel_clone);
                    if target.exists() {
                        std::fs::read(&target)
                            .ok()
                            .and_then(|b| String::from_utf8(b).ok())
                    } else {
                        None
                    }
                }
            };
            (head, working)
        })
        .await
        .map_err(|_| internal("falha ao obter diff"))?;
    Ok(Json(GitDiffView {
        path: rel,
        head,
        working,
    }))
}

// ---- Fora do jail: pedidos dos agentes (F9.5) ----

const MAX_EXEC_COMMAND: usize = 20_000;
/// O cliente refaz o long-poll; abaixo do `--max-time` dele.
const MAX_EXEC_WAIT: Duration = Duration::from_secs(55);

fn exec_error(e: ExecError) -> Response {
    match e {
        ExecError::NotFound => error(StatusCode::NOT_FOUND, "pedido não encontrado"),
        ExecError::NotPending(state) => error(
            StatusCode::CONFLICT,
            &format!("o pedido não está mais pendente ({state:?})").to_lowercase(),
        ),
        ExecError::TooMany => error(
            StatusCode::TOO_MANY_REQUESTS,
            "pedidos demais esperando aprovação; espere uma resposta no app",
        ),
    }
}

/// Sessão dona do `Authorization: Bearer <token>`; 401 sem token, errado ou de sessão morta.
async fn agent_session(state: &AppState, headers: &HeaderMap) -> Result<SessionInfo, Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    match state.0.tmux.session_for_exec_token(token).await {
        Ok(Some(session)) => Ok(session),
        Ok(None) => Err(error(
            StatusCode::UNAUTHORIZED,
            "token ausente ou inválido (a sessão foi encerrada?)",
        )),
        Err(e) => Err(internal(&e)),
    }
}

/// Dispara um pedido aprovado; o resultado fica no store para o long-poll.
fn spawn_exec(state: &AppState, job: ExecJob) {
    let store = state.0.exec.clone();
    tokio::spawn(async move {
        let max_output = store.limits().max_output;
        let result = exec::run_command(&job, max_output).await;
        match &result {
            Ok(r) => tracing::info!(
                "pedido {} terminou: código {:?}{} em {} ms",
                job.id,
                r.exit_code,
                if r.timed_out { " (timeout)" } else { "" },
                r.duration_ms
            ),
            Err(e) => tracing::warn!("pedido {} falhou: {e}", job.id),
        }
        store.finish(&job.id, result);
    });
}

#[derive(Deserialize)]
struct AgentExecRequest {
    command: String,
    /// `$PWD` do agente (mesmo caminho dentro e fora do jail); precisa ficar no projeto.
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

#[derive(Serialize)]
struct AgentExecCreated {
    id: String,
    state: exec::ExecState,
    auto_approved: bool,
}

/// POST /v1/exec (API dos agentes): pede para rodar um comando fora do jail.
async fn agent_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AgentExecRequest>,
) -> Result<(StatusCode, Json<AgentExecCreated>), Response> {
    let session = agent_session(&state, &headers).await?;
    let command = request.command.trim();
    if command.is_empty() || command.len() > MAX_EXEC_COMMAND {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "comando vazio ou grande demais",
        ));
    }
    let dir = sleep_project_dir(&state, &session.profile, &session.project)
        .map_err(|(s, m)| error(s, m))?;
    let cwd = exec::resolve_cwd(&dir, request.cwd.as_deref())
        .map_err(|e| error(StatusCode::BAD_REQUEST, &e))?;
    // Env do perfil como na aba Terminal (sem token: o comando roda fora do jail).
    let spec = SessionSpec {
        profile: session.profile.clone(),
        project: session.project.clone(),
        agent: AgentKind::Shell,
    };
    let plan = LaunchPlan::build(&state.0.config, &state.0.home, &spec, 1)
        .map_err(|e| error(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let auto_approved = load_settings(&state.0.home)
        .map(|file| {
            file.get(&session.profile, &session.project)
                .allowed_commands
                .iter()
                .any(|allowed| allowed.trim() == command)
        })
        .unwrap_or(false);
    let (view, job) = state
        .0
        .exec
        .submit(NewRequest {
            session: session.id.clone(),
            profile: session.profile,
            project: session.project,
            cwd,
            command: command.to_owned(),
            env: plan.env,
            timeout_secs: request.timeout_secs,
            auto_approved,
        })
        .map_err(exec_error)?;
    tracing::info!(
        "pedido {} de {}{}: {}",
        view.id,
        session.id,
        if auto_approved {
            " (sempre permitido)"
        } else {
            ""
        },
        view.command
    );
    if let Some(job) = job {
        spawn_exec(&state, job);
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(AgentExecCreated {
            id: view.id,
            state: view.state,
            auto_approved,
        }),
    ))
}

#[derive(Deserialize)]
struct WaitQuery {
    #[serde(default)]
    wait: u64,
}

/// GET /v1/exec/{id}?wait=<s> (API dos agentes): long-poll até um estado final.
async fn agent_result(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<WaitQuery>,
) -> Result<Json<ExecView>, Response> {
    let session = agent_session(&state, &headers).await?;
    let wait = Duration::from_secs(query.wait).min(MAX_EXEC_WAIT);
    // Pedido de outra sessão: 404, como se não existisse.
    match state.0.exec.wait(&id, wait).await {
        Some(view) if view.session == session.id => Ok(Json(view)),
        _ => Err(exec_error(ExecError::NotFound)),
    }
}

/// GET /api/exec: pendentes e recentes (o app mostra para aprovar).
async fn list_exec(State(state): State<AppState>) -> Json<Vec<ExecView>> {
    Json(state.0.exec.list())
}

#[derive(Deserialize, Default)]
struct ApproveRequest {
    /// Grava o comando em `allowed_commands` do projeto antes de rodar.
    #[serde(default)]
    always: bool,
}

/// POST /api/exec/{id}/approve `{always?}`: roda o pedido fora do jail.
async fn approve_exec(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<ApproveRequest>>,
) -> Result<StatusCode, Response> {
    let always = body.map(|Json(b)| b.always).unwrap_or_default();
    if always {
        let view = state
            .0
            .exec
            .get(&id)
            .ok_or_else(|| exec_error(ExecError::NotFound))?;
        if view.state != exec::ExecState::Pending {
            return Err(exec_error(ExecError::NotPending(view.state)));
        }
        let (profile, project, command) = (view.profile, view.project, view.command);
        let mut current = load_settings(&state.0.home)
            .map_err(|e| internal(&e))?
            .get(&profile, &project);
        if !current.allowed_commands.contains(&command) {
            current.allowed_commands.push(command.clone());
        }
        current.validate().map_err(|e| {
            error(
                StatusCode::BAD_REQUEST,
                &format!("não dá para sempre permitir: {e}"),
            )
        })?;
        update_settings(&state, |file| {
            let mut settings = file.get(&profile, &project);
            if !settings.allowed_commands.contains(&command) {
                settings.allowed_commands.push(command.clone());
            }
            file.set(&profile, &project, settings);
        })
        .await
        .map_err(|e| internal(&e))?;
        tracing::info!("{profile}/{project}: sempre permitir {command:?}");
    }
    let job = state.0.exec.approve(&id).map_err(exec_error)?;
    tracing::info!("pedido {id} aprovado: {}", job.command);
    spawn_exec(&state, job);
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/exec/{id}/deny: o agente recebe "negado pelo usuário".
async fn deny_exec(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, Response> {
    let view = state.0.exec.deny(&id).map_err(exec_error)?;
    tracing::info!("pedido {id} negado: {}", view.command);
    Ok(StatusCode::NO_CONTENT)
}

async fn list_sessions(State(state): State<AppState>) -> Result<Json<Vec<SessionInfo>>, Response> {
    state
        .0
        .tmux
        .list()
        .await
        .map(Json)
        .map_err(|e| internal(&e))
}

async fn create_session(
    State(state): State<AppState>,
    Json(spec): Json<SessionSpec>,
) -> Result<(StatusCode, Json<SessionInfo>), Response> {
    let inner = &state.0;
    let Some(profile) = inner.config.profile(&spec.profile) else {
        return Err(error(StatusCode::NOT_FOUND, "perfil desconhecido"));
    };
    let existing = inner.tmux.list().await.map_err(|e| internal(&e))?;
    let seq = next_seq(&existing, &spec);
    // Arquivo inválido: a sessão nasce com o jail padrão (o mais fechado), não falha.
    let settings = match load_settings(&inner.home) {
        Ok(file) => file.get(&spec.profile, &spec.project),
        Err(e) => {
            tracing::warn!("configurações dos projetos ignoradas: {e}");
            ProjectSettings::default()
        }
    };
    // API dos agentes (F9.5): só com o `agent_listen` no ar e o cliente instalado (senão o
    // `--map` do ai-jail apontaria para uma pasta que não existe). A aba Terminal não precisa.
    let exec = match inner.exec_port {
        Some(port)
            if spec.agent != AgentKind::Shell
                && exec_client_dir(&inner.home).join("fora-do-jail").exists() =>
        {
            match new_exec_token() {
                Ok(token) => Some(ExecAccess { port, token }),
                Err(e) => {
                    tracing::warn!("sessão sem API dos agentes (token: {e})");
                    None
                }
            }
        }
        _ => None,
    };
    let exec_token = exec.as_ref().map(|e| e.token.clone());
    let options = LaunchOptions { settings, exec };
    let mut plan = LaunchPlan::build_with(&inner.config, &inner.home, &spec, seq, &options)
        .map_err(|e| error(StatusCode::BAD_REQUEST, &e.to_string()))?;
    // Só projetos de verdade (repo git na raiz do perfil).
    if !plan.cwd.join(".git").exists() || !plan.cwd.starts_with(&profile.root) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "projeto não encontrado neste perfil",
        ));
    }
    // O ai-jail cria `.ai-jail` na raiz: fora do `git status` antes de a sessão subir (F0.5).
    if let Err(e) = sleep::exclude_jail_config(&plan.cwd).await {
        tracing::warn!("sem exclude do .ai-jail em {}: {e:#}", plan.cwd.display());
    }
    if let Some(argv) = &inner.argv_override {
        plan.argv = argv.clone();
    }
    let info = inner
        .tmux
        .create(&plan, &spec, exec_token.as_deref())
        .await
        .map_err(|e| internal(&e))?;
    tracing::info!("sessão criada: {}", info.id);
    Ok((StatusCode::CREATED, Json(info)))
}

async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, Response> {
    let tmux = &state.0.tmux;
    match tmux.find(&id).await.map_err(|e| internal(&e))? {
        None => Err(error(StatusCode::NOT_FOUND, "sessão não encontrada")),
        Some(_) => {
            tmux.kill(&id).await.map_err(|e| internal(&e))?;
            tracing::info!("sessão encerrada: {id}");
            Ok(StatusCode::NO_CONTENT)
        }
    }
}

async fn open_terminal(
    State(state): State<AppState>,
    Path(id): Path<String>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let tmux = state.0.tmux.clone();
    // Só anexa a sessões do Assistente (o id vira alvo do `tmux attach`).
    match tmux.find(&id).await {
        Ok(Some(_)) => upgrade.on_upgrade(move |socket| terminal::run(socket, tmux, id)),
        Ok(None) => error(StatusCode::NOT_FOUND, "sessão não encontrada"),
        Err(e) => internal(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    const EXAMPLE: &str = include_str!("../../../config/assistente.exemplo.toml");

    fn app(token: Option<&str>) -> Router {
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(EXAMPLE, &home).unwrap();
        router(AppState::new(
            config,
            home,
            token.map(str::to_owned),
            None,
            Tmux::default(),
        ))
    }

    async fn call(app: Router, uri: &str, token: Option<&str>) -> (StatusCode, serde_json::Value) {
        let mut request = Request::get(uri);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn health_is_open() {
        let (status, body) = call(app(Some("s3gredo")), "/api/health", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
    }

    #[tokio::test]
    async fn other_routes_need_the_token() {
        let (status, _) = call(app(Some("s3gredo")), "/api/profiles", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(app(Some("s3gredo")), "/api/profiles", Some("errado")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, body) = call(app(Some("s3gredo")), "/api/profiles", Some("s3gredo")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body[0]["id"], "pessoal");
        assert_eq!(body[1]["id"], "trabalho");
        // Home de exemplo sem `.credentials.json`: nenhum perfil com login do Claude.
        assert_eq!(body[0]["claude_login"], false);
    }

    #[tokio::test]
    async fn unknown_profile_is_404() {
        let (status, _) = call(app(None), "/api/projects?profile=nope", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn missing_profile_root_lists_nothing() {
        let (status, body) = call(app(None), "/api/projects?profile=trabalho", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!([]));
    }

    #[tokio::test]
    async fn projects_do_not_mix_between_profiles() {
        let dir = std::env::temp_dir().join(format!("hubd-perfis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let pessoal = dir.join("pessoal");
        let trabalho = dir.join("trabalho");
        std::fs::create_dir_all(pessoal.join("game/.git")).unwrap();
        std::fs::create_dir_all(trabalho.join("erp/.git")).unwrap();
        let text = EXAMPLE
            .replace(
                "~/Documentos/pessoal/projetos",
                &pessoal.display().to_string(),
            )
            .replace("~/Documentos/trabalho", &trabalho.display().to_string());
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(&text, &home).unwrap();
        let app = router(AppState::new(config, home, None, None, Tmux::default()));

        async fn names(app: Router, profile: &str) -> Vec<String> {
            let (status, body) = call(app, &format!("/api/projects?profile={profile}"), None).await;
            assert_eq!(status, StatusCode::OK);
            body.as_array()
                .unwrap()
                .iter()
                .map(|p| p["name"].as_str().unwrap().to_owned())
                .collect()
        }

        assert_eq!(names(app.clone(), "pessoal").await, ["game"]);
        assert_eq!(names(app, "trabalho").await, ["erp"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ---- Modo dormir de ponta a ponta no devenv (F6.3): tmux local + agente falso ----

    /// O `dorme` de verdade, no caminho absoluto do repo (canonicalize tira os `..`).
    fn dorme_script() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../notebook/dorme");
        std::fs::canonicalize(path).unwrap().display().to_string()
    }

    fn make_agent(dir: &std::path::Path, name: &str, body: &str) -> String {
        let agent = dir.join(name);
        std::fs::write(&agent, format!("#!/usr/bin/env bash\nset -eu\n{body}")).unwrap();
        std::process::Command::new("chmod")
            .arg("+x")
            .arg(&agent)
            .output()
            .unwrap();
        agent.display().to_string()
    }

    fn sleep_app_with_dorme(
        root: &std::path::Path,
        workdir: &std::path::Path,
        agent: &str,
    ) -> Router {
        let text = EXAMPLE
            .replace("~/Documentos/pessoal/projetos", &root.display().to_string())
            .replace("~/assistente", &workdir.display().to_string());
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(&text, &home).unwrap();
        router(
            AppState::new(config, home, None, None, Tmux::default())
                .with_sleep_remote(Remote::Local)
                .with_dorme(
                    dorme_script(),
                    vec![
                        ("AGENTE_OPENCODE".to_owned(), agent.to_owned()),
                        ("AGENTE_JAIL".to_owned(), String::new()),
                        ("AGENTE_ESPERA".to_owned(), "0".to_owned()),
                    ],
                ),
        )
    }

    #[tokio::test]
    async fn sleep_full_cycle_start_status_collect_merge() {
        let dir = scratch("sleep-cycle");
        let root = dir.join("raiz");
        let pc = root.join("proj");
        init_repo(&pc);
        let workdir = dir.join("nb");
        // Agente falso: commita a tarefa e pede parada (o loop encerra na 1ª iteração).
        let agent = make_agent(
            &dir,
            "agente-noite.sh",
            "git config user.email agente@notebook\ngit config user.name Agente\necho trabalho >> feito.txt\ngit add feito.txt\ngit commit -q -m \"feat: tarefa da noite\"\ntouch \"$AGENTE_STOP\"\n",
        );
        let app = || sleep_app_with_dorme(&root, &workdir, &agent);

        // Passo 6: POST /api/sleep → 202 com a sessão e a data da noite.
        let (status, started) = call_post(
            app(),
            "/api/sleep",
            r#"{"profile":"pessoal","project":"proj"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{started}");
        assert_eq!(started["session"], "dorme-proj");
        let date = started["date"].as_str().unwrap().to_owned();
        assert_eq!(date.len(), 10, "{date}");

        // No "notebook": clone na branch de trabalho e prompt renderizado.
        let nb = workdir.join("projetos/pessoal/proj");
        assert_eq!(
            git_sync(&nb, &["branch", "--show-current"]),
            format!("dorme/{date}")
        );
        let prompt = std::fs::read_to_string(workdir.join("briefings/proj.md")).unwrap();
        assert!(prompt.contains("hora de dormir"), "{prompt}");
        assert!(prompt.contains("docs/FUNCIONALIDADES.md"), "{prompt}");
        assert!(
            prompt.contains(&format!("a partir de dorme/{date}")),
            "{prompt}"
        );

        // Passo 7: GET /api/sleep até o agente falso (commit + STOP) encerrar o loop.
        let mut last = serde_json::Value::Null;
        let ended = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let (status, body) =
                    call(app(), "/api/sleep?profile=pessoal&project=proj", None).await;
                assert_eq!(status, StatusCode::OK);
                last = body.clone();
                if body["sleeping"] == false
                    && body["log_tail"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("modo-dormir encerrado")
                {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await
        .unwrap_or(false);
        assert!(ended, "dorme não terminou; último estado: {last}");

        // Passos 8-9: a manhã (F6.5) vê o commit da noite e o merge traz para o PC.
        let request = format!(r#"{{"profile":"pessoal","project":"proj","date":"{date}"}}"#);
        let (status, report) = call_post(app(), "/api/sleep/collect", &request).await;
        assert_eq!(status, StatusCode::OK, "{report}");
        assert_eq!(report["fast_forward"], true);
        assert_eq!(report["commits"][0]["subject"], "feat: tarefa da noite");
        let (status, _) = call_post(app(), "/api/sleep/merge", &request).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            std::fs::read_to_string(pc.join("feito.txt")).unwrap(),
            "trabalho\n"
        );
        assert_eq!(
            git_sync(&pc, &["rev-parse", "HEAD"]),
            git_sync(&nb, &["rev-parse", "HEAD"])
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn sleep_start_guards_dirty_tree_and_duplicate_run() {
        let dir = scratch("sleep-guard");
        let root = dir.join("raiz");
        let pc = root.join("projguarda");
        init_repo(&pc);
        let workdir = dir.join("nb");
        // Agente demorado: dá tempo de ver "rodando" e de recusar a duplicata.
        let agent = make_agent(&dir, "agente-lento.sh", "sleep 30\n");
        let app = || sleep_app_with_dorme(&root, &workdir, &agent);
        let request = r#"{"profile":"pessoal","project":"projguarda"}"#;

        // Árvore suja sem autorização: 409 pedindo commit ou WIP.
        std::fs::write(pc.join("solto.txt"), "x").unwrap();
        let (status, body) = call_post(app(), "/api/sleep", request).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            body["error"].as_str().unwrap().contains("árvore suja"),
            "{body}"
        );

        // Com wip: o que estava solto vira commit WIP e o dormir começa.
        let (status, started) = call_post(
            app(),
            "/api/sleep",
            r#"{"profile":"pessoal","project":"projguarda","wip":true}"#,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{started}");
        assert_eq!(
            git_sync(&pc, &["log", "-1", "--format=%s"]),
            "wip: hora de dormir (auto)"
        );

        // Rodando: GET vê a sessão no tmux e o POST duplicado é recusado.
        let (status, body) =
            call(app(), "/api/sleep?profile=pessoal&project=projguarda", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["sleeping"], true, "{body}");
        let (status, body) = call_post(
            app(),
            "/api/sleep",
            r#"{"profile":"pessoal","project":"projguarda","wip":true}"#,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(
            body["error"].as_str().unwrap().contains("já está rodando"),
            "{body}"
        );

        // F6.4 pela rota: o STOP aparece no workdir do "notebook".
        let response = app()
            .oneshot(
                Request::post("/api/sleep/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(workdir.join("STOP").is_file());

        // Limpeza: encerra o dorme lento.
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", "dorme-projguarda"])
            .output();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn sleep_stop_fails_loudly_without_a_notebook() {
        // No devenv não há ssh/notebook de verdade: a rota tenta e devolve 502 com o motivo,
        // nunca um 501 (o sucesso via ssh é validado na VM real, F6.3).
        let response = app(None)
            .oneshot(
                Request::post("/api/sleep/stop")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("STOP não chegou ao notebook"), "{text}");
    }

    // ---- Configurações do projeto (F9.2) ----

    async fn call_method(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
        token: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_owned()));
        let response = app.oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// Home e raiz do perfil pessoal temporários, com o repo `game`.
    fn settings_app(name: &str, token: Option<&str>) -> (Router, PathBuf) {
        let dir = std::env::temp_dir().join(format!("hubd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("raiz");
        std::fs::create_dir_all(root.join("game/.git")).unwrap();
        let home = dir.join("home");
        let text = EXAMPLE.replace("~/Documentos/pessoal/projetos", &root.display().to_string());
        let config = Config::parse(&text, &home).unwrap();
        let state = AppState::new(
            config,
            home,
            token.map(str::to_owned),
            None,
            Tmux::default(),
        );
        (router(state), dir)
    }

    #[tokio::test]
    async fn project_settings_round_trip() {
        let (app, dir) = settings_app("config-projeto", None);
        let uri = "/api/projects/pessoal/game/settings";
        let (status, body) = call_method(app.clone(), "GET", uri, None, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["settings"]["docker"], false);
        assert_eq!(body["docker_from_project_file"], false);

        let loose = r#"{"docker":true,"allow_hosts":["pypi.org"],"full_network":false,"allowed_commands":["./dev just check"]}"#;
        let (status, body) = call_method(app.clone(), "PUT", uri, Some(loose), None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (_, body) = call_method(app.clone(), "GET", uri, None, None).await;
        assert_eq!(body["settings"]["allow_hosts"], json!(["pypi.org"]));
        assert_eq!(
            body["settings"]["allowed_commands"],
            json!(["./dev just check"])
        );

        let path = settings_path(&dir.join("home"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[pessoal.game]"), "{text}");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        // O `[jail]` do repo aparece separado (compatibilidade).
        std::fs::write(
            dir.join("raiz/game/.assistente.toml"),
            "[jail]\ndocker = true\n",
        )
        .unwrap();
        let (_, body) = call_method(app.clone(), "GET", uri, None, None).await;
        assert_eq!(body["docker_from_project_file"], true);

        // Voltar ao padrão tira o projeto do arquivo.
        let (status, _) = call_method(app, "PUT", uri, Some("{}"), None).await;
        assert_eq!(status, StatusCode::OK);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("game"), "{text}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn project_settings_reject_bad_input() {
        let (app, dir) = settings_app("config-projeto-erros", Some("s3gredo"));
        let token = Some("s3gredo");
        let get = |uri: &'static str, token| call_method(app.clone(), "GET", uri, None, token);
        let (status, _) = get("/api/projects/pessoal/game/settings", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = get("/api/projects/nope/game/settings", token).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = get("/api/projects/pessoal/nao-existe/settings", token).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = get("/api/projects/pessoal/..%2Ffora/settings", token).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = call_method(
            app.clone(),
            "PUT",
            "/api/projects/pessoal/game/settings",
            Some(r#"{"allow_hosts":["https://pypi.org"]}"#),
            token,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(!settings_path(&dir.join("home")).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    // ---- Recolher de manhã (F6.5): collect + merge com repos locais ----

    use crate::sleep::tests::{commit, git_sync, init_repo, scratch};

    async fn call_post(app: Router, uri: &str, body: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(
                Request::post(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_owned()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// hubd com a raiz do perfil pessoal e o workdir do notebook apontando para dirs locais,
    /// e o "notebook" = o próprio host (Remote::Local).
    fn sleep_app(root: &std::path::Path, workdir: &std::path::Path) -> Router {
        let text = EXAMPLE
            .replace("~/Documentos/pessoal/projetos", &root.display().to_string())
            .replace("~/assistente", &workdir.display().to_string());
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(&text, &home).unwrap();
        router(
            AppState::new(config, home, None, None, Tmux::default())
                .with_sleep_remote(Remote::Local),
        )
    }

    #[tokio::test]
    async fn sleep_collect_and_merge_round_trip() {
        let dir = scratch("collect-route");
        let root = dir.join("raiz");
        let pc = root.join("proj");
        init_repo(&pc);
        let workdir = dir.join("nb");
        let remote_dir = workdir.join("projetos/pessoal/proj");

        // Noite: sync (F6.2) + trabalho do agente no "notebook".
        crate::sleep::sync_project(
            &pc,
            &Remote::Local,
            remote_dir.to_str().unwrap(),
            "2026-10-20",
        )
        .await
        .unwrap();
        git_sync(
            &remote_dir,
            &["config", "user.email", "nb@assistente.local"],
        );
        git_sync(&remote_dir, &["config", "user.name", "Notebook"]);
        git_sync(&remote_dir, &["switch", "-q", "-c", "agente/f9.9"]);
        commit(&remote_dir, "feat.txt", "feat: tarefa da noite");
        git_sync(&remote_dir, &["switch", "-q", "dorme/2026-10-20"]);
        git_sync(
            &remote_dir,
            &[
                "merge",
                "-q",
                "--no-ff",
                "agente/f9.9",
                "-m",
                "merge: f9.9 (modo-dormir)",
            ],
        );
        std::fs::write(remote_dir.join("RELATORIO.md"), "fila esgotada\n").unwrap();

        let request = r#"{"profile":"pessoal","project":"proj","date":"2026-10-20"}"#;
        let (status, report) =
            call_post(sleep_app(&root, &workdir), "/api/sleep/collect", request).await;
        assert_eq!(status, StatusCode::OK, "{report}");
        assert_eq!(report["base"], "main");
        assert_eq!(report["sleep_branch"], "dorme/2026-10-20");
        assert_eq!(report["fast_forward"], true);
        assert_eq!(report["commits"][0]["subject"], "merge: f9.9 (modo-dormir)");
        assert_eq!(report["relatorio"], "fila esgotada");

        let (status, _) = call_post(sleep_app(&root, &workdir), "/api/sleep/merge", request).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            git_sync(&pc, &["rev-parse", "HEAD"]),
            git_sync(&remote_dir, &["rev-parse", "HEAD"])
        );
        assert_eq!(
            std::fs::read_to_string(pc.join("feat.txt")).unwrap(),
            "feat: tarefa da noite"
        );

        // A base andou durante a noite: collect avisa (fast_forward false) e o merge
        // recusa com 409 sem mudar nada.
        commit(&pc, "c3.txt", "c3");
        commit(&remote_dir, "c4.txt", "c4");
        let (status, report) =
            call_post(sleep_app(&root, &workdir), "/api/sleep/collect", request).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(report["fast_forward"], false);
        let (status, body) =
            call_post(sleep_app(&root, &workdir), "/api/sleep/merge", request).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(body["error"].as_str().unwrap().contains("merge"), "{body}");
        assert_eq!(git_sync(&pc, &["log", "-1", "--format=%s"]), "c3");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn sleep_routes_reject_bad_profile_and_project() {
        let dir = scratch("sleep-erros");
        let root = dir.join("raiz");
        init_repo(&root.join("proj"));
        let workdir = dir.join("nb");
        let app = || sleep_app(&root, &workdir);
        let request = r#"{"profile":"nope","project":"proj","date":"2026-10-20"}"#;
        let (status, _) = call_post(app(), "/api/sleep/collect", request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let request = r#"{"profile":"pessoal","project":"../fora","date":"2026-10-20"}"#;
        let (status, _) = call_post(app(), "/api/sleep/collect", request).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let request = r#"{"profile":"pessoal","project":"nao-existe","date":"2026-10-20"}"#;
        let (status, _) = call_post(app(), "/api/sleep/merge", request).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ---- Saúde do notebook por HTTP (F4.3) ----

    /// Servidor HTTP mínimo que responde um status fixo; devolve a porta.
    async fn fake_http_server(status: &'static str, body: &'static str) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut request = [0u8; 512];
                let _ = socket.read(&mut request).await;
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        port
    }

    /// Porta livre (nada escutando): bind e descarta o listener.
    async fn closed_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    fn notebook_app(router_port: u16) -> Router {
        let text = EXAMPLE.replace("= 20128", &format!("= {router_port}"));
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(&text, &home).unwrap();
        router(AppState::new(config, home, None, None, Tmux::default()))
    }

    fn memory_app(memory_port: u16) -> Router {
        let text = EXAMPLE.replace("port = 49374", &format!("port = {memory_port}"));
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(&text, &home).unwrap();
        router(AppState::new(config, home, None, None, Tmux::default()))
    }

    fn service<'a>(body: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
        body["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap_or_else(|| panic!("serviço {name} ausente: {body}"))
    }

    #[tokio::test]
    async fn notebook_http_probe_marks_real_services_up() {
        let router = fake_http_server("200 OK", "{\"data\":[]}").await;
        let (status, body) = call(notebook_app(router), "/api/notebook", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["tunnel_managed"], false);
        let nine = service(&body, "9router");
        assert_eq!(nine["up"], true);
        assert_eq!(nine["last_error"], serde_json::Value::Null);
        assert!(nine["checked_at"].as_u64().unwrap() > 0);
        assert_eq!(body["services"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn notebook_http_probe_rejects_false_positives() {
        // Outro processo HTTP na porta do 9router (404 em /v1/models) não é o 9router.
        let impostor = fake_http_server("404 Not Found", "").await;
        let (_, body) = call(notebook_app(impostor), "/api/notebook", None).await;
        let nine = service(&body, "9router");
        assert_eq!(nine["up"], false);
        assert_eq!(nine["last_error"], "HTTP 404 em /v1/models");
    }

    #[tokio::test]
    async fn memory_http_probe_uses_healthz() {
        let port = fake_http_server("200 OK", "{\"status\":\"ok\"}").await;
        let (_, body) = call(memory_app(port), "/api/memory", None).await;
        assert_eq!(body["up"], true);
        assert_eq!(body["port"], port);
        let dead = closed_port().await;
        let (_, body) = call(memory_app(dead), "/api/memory", None).await;
        assert_eq!(body["up"], false);
    }

    // ---- web UI do ai-memory (F5.5) ----

    /// ai-memory falso: responde 200 com o cabeçalho do pedido recebido no corpo.
    async fn echo_http_server() -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.ends_with(b"\r\n\r\n") {
                    match socket.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    head.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(&head).await;
            }
        });
        port
    }

    /// hubd com token `segredo`, ai-memory na `port` e o token do ai-memory em `data_dir`.
    fn web_app(port: u16, name: &str) -> Router {
        let data_dir = crate::sleep::tests::scratch(name);
        std::fs::create_dir_all(data_dir.join("auth-tokens")).unwrap();
        std::fs::write(data_dir.join("auth-tokens/default"), "token-memoria\n").unwrap();
        let text = EXAMPLE
            .replace("port = 49374", &format!("port = {port}"))
            .replace(
                "data_dir = \"~/.local/share/assistente/ai-memory\"",
                &format!("data_dir = \"{}\"", data_dir.display()),
            );
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(&text, &home).unwrap();
        router(AppState::new(
            config,
            home,
            Some("segredo".into()),
            None,
            Tmux::default(),
        ))
    }

    async fn get_web(app: Router, uri: &str, headers: &[(header::HeaderName, String)]) -> Response {
        let mut request = Request::get(uri);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        app.oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn body_text(response: Response) -> String {
        let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        String::from_utf8_lossy(&body).into_owned()
    }

    #[tokio::test]
    async fn memory_web_requires_hub_token_or_cookie() {
        let app = web_app(echo_http_server().await, "web-sem-token");
        let response = get_web(app.clone(), "/web", &[]).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = get_web(app.clone(), "/memoria/entrar?token=errado", &[]).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let forged = [(header::COOKIE, format!("{MEMORY_COOKIE}=chute"))];
        let response = get_web(app, "/api/v1/projects", &forged).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn memory_login_cookie_reaches_ai_memory_with_its_token() {
        let app = web_app(echo_http_server().await, "web-cookie");
        let response = get_web(app.clone(), "/memoria/entrar?token=segredo", &[]).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()[header::LOCATION], "/web");
        let set_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(set_cookie.contains("HttpOnly"), "{set_cookie}");
        let cookie = set_cookie.split(';').next().unwrap().to_owned();
        assert!(
            !cookie.contains("segredo"),
            "o cookie não é o token do hub: {cookie}"
        );

        let response = get_web(app, "/web/projects?x=1", &[(header::COOKIE, cookie)]).await;
        assert_eq!(response.status(), StatusCode::OK);
        let upstream = body_text(response).await;
        assert!(upstream.starts_with("GET /web/projects?x=1 "), "{upstream}");
        assert!(
            upstream.contains("authorization: Bearer token-memoria"),
            "{upstream}"
        );
    }

    #[tokio::test]
    async fn memory_proxy_swaps_hub_token_for_ai_memory_token() {
        let app = web_app(echo_http_server().await, "web-bearer");
        let hub = [(header::AUTHORIZATION, "Bearer segredo".to_owned())];
        let upstream = body_text(get_web(app, "/api/v1/projects", &hub).await).await;
        assert!(
            upstream.contains("authorization: Bearer token-memoria"),
            "{upstream}"
        );
        assert!(
            !upstream.contains("segredo"),
            "token do hub vazou: {upstream}"
        );
    }

    // ---- hubd serve a UI (F8.3) ----

    /// Como o `call`, mas devolve o corpo em texto (HTML/JS não são JSON).
    async fn call_text(app: Router, uri: &str) -> (StatusCode, String) {
        let response = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    fn ui_app(dir: Option<PathBuf>) -> Router {
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(EXAMPLE, &home).unwrap();
        router(
            AppState::new(
                config,
                home,
                Some("s3gredo".to_owned()),
                None,
                Tmux::default(),
            )
            .with_ui_dir(dir),
        )
    }

    #[tokio::test]
    async fn serves_the_built_ui_without_token() {
        let dir = std::env::temp_dir().join(format!("hubd-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        // Como o build do vite com base "./": o index.html referencia o asset por caminho
        // relativo, que sob /ui/ resolve para o próprio /ui/.
        std::fs::write(
            dir.join("index.html"),
            r#"<!doctype html><script src="./assets/app.js"></script>"#,
        )
        .unwrap();
        std::fs::write(dir.join("assets/app.js"), "console.log('ui do hubd');").unwrap();
        std::fs::write(dir.join("commit.txt"), "abc1234\n").unwrap();

        // Aberto, mesmo com token configurado: o bundle não tem segredo.
        let (status, body) = call_text(ui_app(Some(dir.clone())), "/ui/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("./assets/app.js"), "{body}");

        // O asset que o index.html aponta também é servido.
        let (status, body) = call_text(ui_app(Some(dir.clone())), "/ui/assets/app.js").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("ui do hubd"), "{body}");

        // Escapar da pasta da UI não revela o resto do disco.
        let (status, _) = call_text(ui_app(Some(dir.clone())), "/ui/../../Cargo.toml").await;
        assert!(matches!(
            status,
            StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST
        ));

        // /api/health diz que commit de UI está servido (e o do próprio hubd).
        let (status, body) = call(ui_app(Some(dir.clone())), "/api/health", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["commit"]["ui"], "abc1234");
        assert!(!body["commit"]["hubd"].as_str().unwrap().is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn without_ui_dir_the_route_is_gone_and_commit_is_null() {
        let (status, _) = call_text(ui_app(None), "/ui/").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (_, body) = call(ui_app(None), "/api/health", None).await;
        assert_eq!(body["commit"]["ui"], serde_json::Value::Null);
    }

    // ---- Sessões e terminal com tmux real (socket isolado) ----

    use crate::sessions::tests::TestTmux;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    /// hubd de verdade numa porta livre, com um perfil cuja raiz tem o repo `proj`.
    async fn live_hub(t: &TestTmux, token: Option<&str>) -> String {
        let root = t.dir.join("raiz");
        std::fs::create_dir_all(root.join("proj/.git")).unwrap();
        let text = EXAMPLE.replace("~/Documentos/pessoal/projetos", &root.display().to_string());
        let home = PathBuf::from("/home/rafa");
        let config = Config::parse(&text, &home).unwrap();
        let state = AppState::new(config, home, token.map(str::to_owned), None, t.tmux.clone())
            .with_argv_override(vec!["bash".into(), "--norc".into(), "-i".into()]);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        format!("127.0.0.1:{addr}", addr = addr.port())
    }

    async fn http(method: &str, url: &str, body: Option<&str>) -> (u16, String) {
        http_as(method, url, body, None).await
    }

    async fn http_as(
        method: &str,
        url: &str,
        body: Option<&str>,
        bearer: Option<&str>,
    ) -> (u16, String) {
        // Cliente HTTP mínimo por cima do tokio (sem crate extra).
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (host, path) = url
            .split_once('/')
            .map(|(h, p)| (h, format!("/{p}")))
            .unwrap();
        let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
        let body = body.unwrap_or("");
        let auth = bearer
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        let status = response[9..12].parse().unwrap();
        let body = response
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_owned())
            .unwrap_or_default();
        (status, body)
    }

    #[tokio::test]
    async fn session_lifecycle_and_terminal() {
        let t = TestTmux::new("api");
        let hub = live_hub(&t, None).await;
        let spec = r#"{"profile":"pessoal","project":"proj","agent":"claude"}"#;

        let (status, body) = http("POST", &format!("{hub}/api/sessions"), Some(spec)).await;
        assert_eq!(status, 201, "{body}");
        let info: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(info["id"], "pessoal--proj--claude-1");
        // Segunda sessão do mesmo agente ganha o próximo número.
        let (_, body) = http("POST", &format!("{hub}/api/sessions"), Some(spec)).await;
        assert!(body.contains("pessoal--proj--claude-2"), "{body}");

        // Terminal: o que digitamos é executado e a saída volta.
        let url = format!("ws://{hub}/api/sessions/pessoal--proj--claude-1/terminal");
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        ws.send(WsMessage::Text(
            r#"{"type":"resize","cols":100,"rows":30}"#.into(),
        ))
        .await
        .unwrap();
        ws.send(WsMessage::Text(
            r#"{"type":"input","data":"echo marca-$((40+2))\r"}"#.into(),
        ))
        .await
        .unwrap();
        let mut seen = String::new();
        let found = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(Ok(message)) = ws.next().await {
                if let WsMessage::Binary(bytes) = message {
                    seen.push_str(&String::from_utf8_lossy(&bytes));
                    if seen.contains("marca-42") {
                        return true;
                    }
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        assert!(found, "saída do terminal sem marca-42:\n{seen}");

        // Acentos chegam inteiros no agente e voltam inteiros (locale UTF-8 no attach e no painel).
        ws.send(WsMessage::Text(
            r#"{"type":"input","data":"echo joão-$((40+2))\r"}"#.into(),
        ))
        .await
        .unwrap();
        let found = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(Ok(message)) = ws.next().await {
                if let WsMessage::Binary(bytes) = message {
                    seen.push_str(&String::from_utf8_lossy(&bytes));
                    if seen.contains("joão-42") {
                        return true;
                    }
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        assert!(found, "saída do terminal sem joão-42:\n{seen}");

        // Um TUI que copia por OSC 52 (o opencode prende o mouse e copia sozinho) chega ao app.
        // O base64 é gerado no shell para não aparecer no eco do que digitamos.
        ws.send(WsMessage::Text(
            r#"{"type":"input","data":"printf '\\033]52;c;%s\\007' $(printf copia-%s 42 | base64)\r"}"#.into(),
        ))
        .await
        .unwrap();
        let copied = "Y29waWEtNDI="; // base64("copia-42")
        let found = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(Ok(message)) = ws.next().await {
                if let WsMessage::Binary(bytes) = message {
                    seen.push_str(&String::from_utf8_lossy(&bytes));
                    if seen.contains(copied) {
                        return true;
                    }
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        assert!(found, "OSC 52 do painel não chegou ao terminal:\n{seen:?}");
        drop(ws);

        // Fechar o terminal não mata a sessão.
        let (_, body) = http("GET", &format!("{hub}/api/sessions"), None).await;
        assert!(body.contains("pessoal--proj--claude-1"), "{body}");

        let (status, _) = http(
            "DELETE",
            &format!("{hub}/api/sessions/pessoal--proj--claude-1"),
            None,
        )
        .await;
        assert_eq!(status, 204);
        let (status, _) = http(
            "DELETE",
            &format!("{hub}/api/sessions/pessoal--proj--claude-1"),
            None,
        )
        .await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn terminal_tab_is_a_shell_session() {
        let t = TestTmux::new("api-shell");
        let hub = live_hub(&t, None).await;
        let spec = r#"{"profile":"pessoal","project":"proj","agent":"shell"}"#;
        let (status, body) = http("POST", &format!("{hub}/api/sessions"), Some(spec)).await;
        assert_eq!(status, 201, "{body}");
        let info: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(info["id"], "pessoal--proj--shell-1");
        assert_eq!(info["agent"], "shell");
        let (_, body) = http("GET", &format!("{hub}/api/sessions"), None).await;
        assert!(body.contains(r#""agent":"shell""#), "{body}");
    }

    // ---- Fora do jail: pedidos dos agentes (F9.5) ----

    struct ExecHub {
        /// API do app (7878 na VM).
        hub: String,
        /// API dos agentes (`agent_listen`).
        agents: String,
        home: PathBuf,
    }

    /// hubd com as duas APIs, home temporário (com o cliente `fora-do-jail` "instalado") e o
    /// repo `proj`.
    async fn live_exec_hub(t: &TestTmux, store: ExecStore) -> ExecHub {
        let root = t.dir.join("raiz");
        std::fs::create_dir_all(root.join("proj/.git")).unwrap();
        let home = t.dir.join("home");
        let client = exec_client_dir(&home);
        std::fs::create_dir_all(&client).unwrap();
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../vm/fora-do-jail"),
            client.join("fora-do-jail"),
        )
        .unwrap();
        let text = EXAMPLE.replace("~/Documentos/pessoal/projetos", &root.display().to_string());
        let config = Config::parse(&text, &home).unwrap();
        let agents = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let agents_port = agents.local_addr().unwrap().port();
        let state = AppState::new(config, home.clone(), None, None, t.tmux.clone())
            .with_argv_override(vec!["bash".into(), "--norc".into(), "-i".into()])
            .with_exec_store(store)
            .with_exec_port(Some(agents_port));
        let main = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let main_port = main.local_addr().unwrap().port();
        let agent_state = state.clone();
        tokio::spawn(async move {
            axum::serve(agents, agent_router(agent_state))
                .await
                .unwrap()
        });
        tokio::spawn(async move { axum::serve(main, router(state)).await.unwrap() });
        ExecHub {
            hub: format!("127.0.0.1:{main_port}"),
            agents: format!("127.0.0.1:{agents_port}"),
            home,
        }
    }

    async fn new_session(hub: &str, agent: &str) -> String {
        let spec = format!(r#"{{"profile":"pessoal","project":"proj","agent":"{agent}"}}"#);
        let (status, body) = http("POST", &format!("{hub}/api/sessions"), Some(&spec)).await;
        assert_eq!(status, 201, "{body}");
        let info: serde_json::Value = serde_json::from_str(&body).unwrap();
        info["id"].as_str().unwrap().to_owned()
    }

    async fn exec_token(t: &TestTmux, session: &str) -> String {
        let target = format!("={session}:");
        let out = t
            .tmux
            .command(&[
                "show-options".into(),
                "-v".into(),
                "-t".into(),
                target,
                "@assistente_exec_token".into(),
            ])
            .output()
            .await
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn json(body: &str) -> serde_json::Value {
        serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"))
    }

    #[tokio::test]
    async fn agent_asks_user_approves_and_gets_the_output() {
        let t = TestTmux::new("api-exec");
        let store = ExecStore::new(exec::ExecLimits {
            max_pending_per_session: 2,
            ..exec::ExecLimits::default()
        });
        let ExecHub { hub, agents, home } = live_exec_hub(&t, store).await;
        let session = new_session(&hub, "claude").await;
        let token = exec_token(&t, &session).await;
        assert_eq!(token.len(), 64, "sessão de agente ganha token");
        // O token não vai para a lista de sessões que a UI recebe.
        let (_, listed) = http("GET", &format!("{hub}/api/sessions"), None).await;
        assert!(!listed.contains(&token));

        let submit = |body: &'static str, bearer: Option<String>| {
            let url = format!("{agents}/v1/exec");
            async move { http_as("POST", &url, Some(body), bearer.as_deref()).await }
        };
        let tok = || Some(token.clone());
        assert_eq!(submit(r#"{"command":"true"}"#, None).await.0, 401);
        assert_eq!(
            submit(r#"{"command":"true"}"#, Some("errado".into()))
                .await
                .0,
            401
        );
        assert_eq!(submit(r#"{"command":"  "}"#, tok()).await.0, 400);
        assert_eq!(
            submit(r#"{"command":"true","cwd":"/etc"}"#, tok()).await.0,
            400
        );

        // Pedido → pendente no app → aprovado → saída volta ao agente.
        let (status, body) = submit(
            r#"{"command":"echo marca-$((40+2)) em $(basename $PWD) $GIT_AUTHOR_EMAIL"}"#,
            tok(),
        )
        .await;
        assert_eq!(status, 202, "{body}");
        let id = json(&body)["id"].as_str().unwrap().to_owned();
        assert_eq!(json(&body)["state"], "pending");
        let (_, body) = http("GET", &format!("{hub}/api/exec"), None).await;
        let pending = json(&body);
        assert_eq!(pending[0]["id"], id.as_str());
        assert_eq!(pending[0]["session"], session.as_str());
        assert_eq!(pending[0]["state"], "pending");
        let result_url = |wait: u32| format!("{agents}/v1/exec/{id}?wait={wait}");
        let (_, body) = http_as("GET", &result_url(1), None, tok().as_deref()).await;
        assert_eq!(json(&body)["state"], "pending");

        let approve = format!("{hub}/api/exec/{id}/approve");
        assert_eq!(http("POST", &approve, Some("{}")).await.0, 204);
        assert_eq!(http("POST", &approve, Some("{}")).await.0, 409);
        let (_, body) = http_as("GET", &result_url(10), None, tok().as_deref()).await;
        let done = json(&body);
        assert_eq!(done["state"], "done", "{done}");
        assert_eq!(done["result"]["exit_code"], 0);
        assert_eq!(
            done["result"]["stdout"],
            "marca-42 em proj voce@exemplo.com\n"
        );

        // Negado.
        let (_, body) = submit(r#"{"command":"rm -rf /tmp/nada"}"#, tok()).await;
        let denied = json(&body)["id"].as_str().unwrap().to_owned();
        assert_eq!(
            http("POST", &format!("{hub}/api/exec/{denied}/deny"), None)
                .await
                .0,
            204
        );
        let (_, body) = http_as(
            "GET",
            &format!("{agents}/v1/exec/{denied}?wait=5"),
            None,
            tok().as_deref(),
        )
        .await;
        assert_eq!(json(&body)["state"], "denied");

        // "Sempre permitir": grava no projetos.toml e o próximo igual roda sozinho.
        let (_, body) = submit(r#"{"command":"echo sempre"}"#, tok()).await;
        let first = json(&body)["id"].as_str().unwrap().to_owned();
        let always = format!("{hub}/api/exec/{first}/approve");
        assert_eq!(
            http("POST", &always, Some(r#"{"always":true}"#)).await.0,
            204
        );
        let saved = std::fs::read_to_string(settings_path(&home)).unwrap();
        assert!(saved.contains("echo sempre"), "{saved}");
        let (status, body) = submit(r#"{"command":"echo sempre"}"#, tok()).await;
        assert_eq!(status, 202);
        assert_eq!(json(&body)["auto_approved"], true);
        let auto = json(&body)["id"].as_str().unwrap().to_owned();
        let (_, body) = http_as(
            "GET",
            &format!("{agents}/v1/exec/{auto}?wait=10"),
            None,
            tok().as_deref(),
        )
        .await;
        assert_eq!(json(&body)["result"]["stdout"], "sempre\n");

        // Outra sessão não lê o pedido desta; limite de pendentes por sessão.
        let other = new_session(&hub, "opencode").await;
        let other_token = exec_token(&t, &other).await;
        let (status, _) = http_as("GET", &result_url(0), None, Some(&other_token)).await;
        assert_eq!(status, 404);
        let other_submit = |cmd: &'static str| {
            let url = format!("{agents}/v1/exec");
            let token = other_token.clone();
            async move { http_as("POST", &url, Some(cmd), Some(&token)).await.0 }
        };
        assert_eq!(other_submit(r#"{"command":"a"}"#).await, 202);
        assert_eq!(other_submit(r#"{"command":"b"}"#).await, 202);
        assert_eq!(other_submit(r#"{"command":"c"}"#).await, 429);

        // A aba Terminal não ganha token.
        let shell = new_session(&hub, "shell").await;
        assert_eq!(exec_token(&t, &shell).await, "");

        // Cada API no seu listener.
        assert_eq!(
            http("GET", &format!("{agents}/api/sessions"), None).await.0,
            404
        );
        assert_eq!(
            http("GET", &format!("{agents}/api/exec"), None).await.0,
            404
        );
        assert_eq!(
            http("POST", &format!("{hub}/v1/exec"), Some("{}")).await.0,
            404
        );

        // Sessão encerrada: o token morre junto.
        assert_eq!(
            http("DELETE", &format!("{hub}/api/sessions/{session}"), None)
                .await
                .0,
            204
        );
        assert_eq!(submit(r#"{"command":"true"}"#, tok()).await.0, 401);
    }

    /// Aprova (ou nega) o primeiro pedido pendente assim que ele aparecer no app.
    async fn answer_first_pending(hub: String, verb: &'static str) {
        for _ in 0..100 {
            let (_, body) = http("GET", &format!("{hub}/api/exec"), None).await;
            if let Some(id) = json(&body)
                .as_array()
                .and_then(|all| all.iter().find(|v| v["state"] == "pending"))
                .and_then(|v| v["id"].as_str())
            {
                let url = format!("{hub}/api/exec/{id}/{verb}");
                assert_eq!(http("POST", &url, Some("{}")).await.0, 204);
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("nenhum pedido apareceu no app");
    }

    /// O cliente que o agente usa no jail (`vm/fora-do-jail`) contra o hubd de verdade.
    async fn run_client(
        t: &TestTmux,
        agents: &str,
        token: &str,
        args: &[&str],
    ) -> std::process::Output {
        tokio::process::Command::new("bash")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../vm/fora-do-jail"
            ))
            .args(args)
            .current_dir(t.dir.join("raiz/proj"))
            .env("ASSISTENTE_EXEC_URL", format!("http://{agents}"))
            .env("ASSISTENTE_EXEC_TOKEN", token)
            .output()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn client_script_round_trip() {
        let t = TestTmux::new("api-cliente");
        let ExecHub { hub, agents, .. } = live_exec_hub(&t, ExecStore::default()).await;
        let session = new_session(&hub, "opencode").await;
        let token = exec_token(&t, &session).await;

        let approver = tokio::spawn(answer_first_pending(hub.clone(), "approve"));
        let out = run_client(
            &t,
            &agents,
            &token,
            &["--", "echo marca-$((40+2)); echo aviso >&2; exit 7"],
        )
        .await;
        approver.await.unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "marca-42\n",
            "{stderr}"
        );
        assert!(stderr.contains("aguardando aprovação"), "{stderr}");
        assert!(stderr.contains("aviso"), "{stderr}");
        assert_eq!(out.status.code(), Some(7), "{stderr}");

        let denier = tokio::spawn(answer_first_pending(hub.clone(), "deny"));
        let out = run_client(&t, &agents, &token, &["rm", "-rf", "build"]).await;
        denier.await.unwrap();
        assert_eq!(out.status.code(), Some(126));
        assert!(String::from_utf8_lossy(&out.stderr).contains("negado"));

        // Fora de uma sessão (sem env) e com token morto.
        let out = run_client(&t, &agents, "", &["true"]).await;
        assert_eq!(out.status.code(), Some(2));
        let out = run_client(&t, &agents, "token-errado", &["true"]).await;
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("token recusado"));
        // Pedido que sumiu (hubd reiniciou): avisa para pedir de novo.
        let out = run_client(&t, &agents, &token, &["--resultado", "nao-existe"]).await;
        assert_eq!(out.status.code(), Some(125));
        assert!(String::from_utf8_lossy(&out.stderr).contains("peça de novo"));
    }

    #[tokio::test]
    async fn create_rejects_missing_project_and_terminal_needs_token() {
        let t = TestTmux::new("api-erros");
        let hub = live_hub(&t, Some("s3gredo")).await;
        let (status, _) = http("GET", &format!("{hub}/api/sessions"), None).await;
        assert_eq!(status, 401);
        // Terminal sem token e com token errado: recusado antes de qualquer upgrade.
        let base = format!("ws://{hub}/api/sessions/x/terminal");
        assert!(
            tokio_tungstenite::connect_async(base.clone())
                .await
                .is_err()
        );
        assert!(
            tokio_tungstenite::connect_async(format!("{base}?token=errado"))
                .await
                .is_err()
        );

        let t2 = TestTmux::new("api-sem-projeto");
        let hub2 = live_hub(&t2, None).await;
        let spec = r#"{"profile":"pessoal","project":"nao-existe","agent":"opencode"}"#;
        let (status, _) = http("POST", &format!("{hub2}/api/sessions"), Some(spec)).await;
        assert_eq!(status, 404);
        let spec = r#"{"profile":"pessoal","project":"../fora","agent":"opencode"}"#;
        let (status, _) = http("POST", &format!("{hub2}/api/sessions"), Some(spec)).await;
        assert_eq!(status, 400);
    }

    // ---- Arquivos e git do projeto (F10.1) ----

    /// Home e raiz do perfil pessoal temporários, com o repo `game` inicializado.
    fn files_app(name: &str, token: Option<&str>) -> (Router, PathBuf) {
        let dir = std::env::temp_dir().join(format!("hubd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("raiz");
        let pc = root.join("game");
        init_repo(&pc);
        let home = dir.join("home");
        let text = EXAMPLE.replace("~/Documentos/pessoal/projetos", &root.display().to_string());
        let config = Config::parse(&text, &home).unwrap();
        let state = AppState::new(
            config,
            home,
            token.map(str::to_owned),
            None,
            Tmux::default(),
        );
        (router(state), dir)
    }

    #[tokio::test]
    async fn files_list_reads_repo_tree() {
        let (app, dir) = files_app("files-list", None);
        // Cria um arquivo não rastreado e um ignorado.
        std::fs::write(dir.join("raiz/game/b.txt"), "2").unwrap();
        std::fs::write(dir.join("raiz/game/.gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(dir.join("raiz/game/ignored.txt"), "x").unwrap();
        git_sync(&dir.join("raiz/game"), &["add", ".gitignore"]);
        git_sync(&dir.join("raiz/game"), &["commit", "-q", "-m", "ignore"]);

        let (status, body) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/game/files",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let files = body["files"].as_array().unwrap();
        let names: Vec<&str> = files.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"a.txt"), "{names:?}");
        assert!(names.contains(&"b.txt"), "{names:?}");
        assert!(names.contains(&".gitignore"), "{names:?}");
        assert!(!names.contains(&"ignored.txt"), "{names:?}");
        assert!(
            !names.iter().any(|n| n == &".git" || n.starts_with(".git/")),
            "{names:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn files_read_write_round_trip() {
        let (app, dir) = files_app("files-rw", None);
        let uri = "/api/projects/pessoal/game/files/a.txt";
        let (status, body) = call_method(app.clone(), "GET", uri, None, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["content"], "1");

        let (status, _) =
            call_method(app.clone(), "PUT", uri, Some(r#"{"content":"novo"}"#), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, body) = call_method(app.clone(), "GET", uri, None, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["content"], "novo");
        assert_eq!(
            std::fs::read_to_string(dir.join("raiz/game/a.txt")).unwrap(),
            "novo"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn files_reject_bad_paths_and_binary() {
        let (app, dir) = files_app("files-bad", Some("s3gredo"));
        let token = Some("s3gredo");

        // path com ..
        let (status, _) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/game/files/..%2Ffora",
            None,
            token,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // inexistente
        let (status, _) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/game/files/nao-existe.txt",
            None,
            token,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // binário
        std::fs::write(dir.join("raiz/game/bin.bin"), [0u8, 1, 2]).unwrap();
        let (status, _) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/game/files/bin.bin",
            None,
            token,
        )
        .await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);

        // token ausente
        let (status, _) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/game/files/a.txt",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn files_write_rejects_symlink_escape() {
        let (app, dir) = files_app("files-symlink", None);
        // symlink dentro do projeto apontando para fora
        #[cfg(unix)]
        {
            let outside = dir.join("fora.txt");
            std::fs::write(&outside, "segredo").unwrap();
            let _ = std::os::unix::fs::symlink(&outside, dir.join("raiz/game/escape.txt"));
            let (status, _) = call_method(
                app.clone(),
                "GET",
                "/api/projects/pessoal/game/files/escape.txt",
                None,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn git_status_reports_changes() {
        let (app, dir) = files_app("git-status", None);
        // modifica tracked, cria novo, apaga tracked
        std::fs::write(dir.join("raiz/game/a.txt"), "modificado").unwrap();
        std::fs::write(dir.join("raiz/game/novo.txt"), "novo").unwrap();
        std::fs::remove_file(dir.join("raiz/game/a.txt")).ok();
        // recria a.txt modificado em vez de apagado para testar status modified
        std::fs::write(dir.join("raiz/game/a.txt"), "modificado").unwrap();

        let (status, body) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/game/git/status",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let files = body["files"].as_array().unwrap();
        // novo.txt aparece como untracked
        assert!(files.iter().any(|f| f["path"] == "novo.txt"), "{body}");
        // a.txt aparece como modified
        assert!(
            files
                .iter()
                .any(|f| f["path"] == "a.txt" && f["status"] == "modified"),
            "{body}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn git_diff_returns_head_and_working() {
        let (app, dir) = files_app("git-diff", None);
        std::fs::write(dir.join("raiz/game/a.txt"), "alterado").unwrap();
        let (status, body) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/game/git/diff/a.txt",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["head"], "1");
        assert_eq!(body["working"], "alterado");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn git_diff_handles_new_file() {
        let (app, dir) = files_app("git-diff-new", None);
        std::fs::write(dir.join("raiz/game/novo.txt"), "conteudo").unwrap();
        let (status, body) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/game/git/diff/novo.txt",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["head"], serde_json::Value::Null);
        assert_eq!(body["working"], "conteudo");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn routes_reject_bad_profile_and_project() {
        let (app, dir) = files_app("files-routes", None);
        let (status, _) = call_method(
            app.clone(),
            "GET",
            "/api/projects/nope/game/files",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/nao-existe/files",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = call_method(
            app.clone(),
            "GET",
            "/api/projects/pessoal/../files",
            None,
            None,
        )
        .await;
        // profile inválido → 404 (perfil desconhecido)
        assert!(status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
