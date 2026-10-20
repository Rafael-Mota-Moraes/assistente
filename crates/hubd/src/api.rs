//! Rotas HTTP do hubd. Só `/api/health` é aberta; o resto exige `Authorization: Bearer <token>`
//! (no WebSocket do terminal, que não manda header, vale `?token=`).
//! Rotas ainda não implementadas respondem 501 com o ID da funcionalidade (docs/FUNCIONALIDADES.md).

use axum::extract::{Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use hub_core::{
    Config, LaunchOptions, LaunchPlan, Project, ProjectSettings, SessionSpec, discover_projects,
    load_settings, settings_path,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::sessions::{SessionInfo, Tmux, next_seq};
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
        }))
    }

    /// Serve a UI de `dir` (`None` desliga o `/ui/`). Antes de compartilhar o estado.
    pub fn with_ui_dir(self, dir: Option<PathBuf>) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.ui_dir = dir;
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
        .route("/api/notebook", get(notebook))
        .route("/api/memory", get(memory))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", delete(delete_session))
        .route("/api/sessions/{id}/terminal", get(open_terminal))
        .route("/api/sleep", get(sleep_status).post(start_sleep))
        .route("/api/sleep/stop", post(stop_sleep))
        .route("/api/sleep/collect", post(collect_sleep))
        .route("/api/sleep/merge", post(merge_sleep))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    let mut public = Router::new().route("/api/health", get(health));
    // O bundle da UI não tem segredo: `/ui/*` é aberto (spec 09, F8.3).
    if let Some(dir) = ui_dir {
        public = public.nest_service("/ui", ServeDir::new(dir));
    }

    public.merge(protected).layer(cors()).with_state(state)
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
    mirror_dir: String,
    up: bool,
    last_error: Option<String>,
    checked_at: u64,
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
    Json(MemoryStatus {
        port,
        mirror_dir: memory.mirror_dir.display().to_string(),
        up,
        last_error,
        checked_at,
    })
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
    sleep::collect_project(&dir, &state.sleep_remote(), &remote_dir, &request.date)
        .await
        .map(Json)
        .map_err(|e| {
            error(
                StatusCode::BAD_GATEWAY,
                &format!("não consegui recolher do notebook: {e}"),
            )
        })
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
    let options = LaunchOptions { settings };
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
        .create(&plan, &spec)
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
        // Cliente HTTP mínimo por cima do tokio (sem crate extra).
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (host, path) = url
            .split_once('/')
            .map(|(h, p)| (h, format!("/{p}")))
            .unwrap();
        let mut stream = tokio::net::TcpStream::connect(host).await.unwrap();
        let body = body.unwrap_or("");
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
}
