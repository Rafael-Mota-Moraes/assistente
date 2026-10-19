//! Rotas HTTP do hubd. Só `/api/health` é aberta; o resto exige `Authorization: Bearer <token>`
//! (no WebSocket do terminal, que não manda header, vale `?token=`).
//! Rotas ainda não implementadas respondem 501 com o ID da funcionalidade (docs/FUNCIONALIDADES.md).

use axum::extract::{Path, Query, Request, State, WebSocketUpgrade};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use hub_core::{Config, LaunchPlan, Project, SessionSpec, discover_projects};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;

use crate::sessions::{SessionInfo, Tmux, next_seq};
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
}

impl AppState {
    pub fn new(
        config: Config,
        home: PathBuf,
        token: Option<String>,
        tunnel: Option<TunnelHandle>,
        tmux: Tmux,
    ) -> Self {
        Self(Arc::new(Inner {
            config,
            home,
            token,
            started: Instant::now(),
            tunnel,
            tmux,
            argv_override: None,
        }))
    }

    #[cfg(test)]
    fn with_argv_override(self, argv: Vec<String>) -> Self {
        let mut inner = Arc::try_unwrap(self.0)
            .ok()
            .expect("estado ainda não compartilhado");
        inner.argv_override = Some(argv);
        Self(Arc::new(inner))
    }
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/profiles", get(profiles))
        .route("/api/projects", get(projects))
        .route("/api/notebook", get(notebook))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route("/api/sessions/{id}", delete(delete_session))
        .route("/api/sessions/{id}/terminal", get(open_terminal))
        .route(
            "/api/sleep",
            get(|| not_yet("F6.3")).post(|| not_yet("F6.3")),
        )
        .route("/api/sleep/stop", post(|| not_yet("F6.4")))
        .route("/api/sleep/collect", post(|| not_yet("F6.5")))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    Router::new()
        .route("/api/health", get(health))
        .merge(protected)
        .layer(cors())
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
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
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

async fn not_yet(feature: &'static str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({ "error": "ainda não implementado", "feature": feature })),
    )
        .into_response()
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    version: &'static str,
    uptime_seconds: u64,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        ok: true,
        version: env!("CARGO_PKG_VERSION"),
        uptime_seconds: state.0.started.elapsed().as_secs(),
    })
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
}

#[derive(Serialize)]
struct NotebookStatus {
    ssh_host: String,
    /// false com `hubd --no-tunnel`: aí só testamos as portas locais.
    tunnel_managed: bool,
    tunnel: Option<TunnelStatus>,
    services: Vec<ServiceStatus>,
}

/// Um serviço do notebook está "de pé" se a porta encaminhada responde em 127.0.0.1.
/// TODO(F4.3): checar por HTTP; outro processo na mesma porta dá falso positivo.
async fn notebook(State(state): State<AppState>) -> Json<NotebookStatus> {
    let notebook = &state.0.config.notebook;
    let mut services = Vec::new();
    for (name, &port) in &notebook.tunnel {
        let up = tokio::time::timeout(
            Duration::from_millis(500),
            tokio::net::TcpStream::connect(("127.0.0.1", port)),
        )
        .await
        .is_ok_and(|r| r.is_ok());
        services.push(ServiceStatus {
            name: name.clone(),
            port,
            up,
        });
    }
    Json(NotebookStatus {
        ssh_host: notebook.ssh_host.clone(),
        tunnel_managed: state.0.tunnel.is_some(),
        tunnel: state.0.tunnel.as_ref().map(TunnelHandle::status),
        services,
    })
}

fn internal(message: &str) -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, message)
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
    let mut plan = LaunchPlan::build(&inner.config, &inner.home, &spec, seq)
        .map_err(|e| error(StatusCode::BAD_REQUEST, &e.to_string()))?;
    // Só projetos de verdade (repo git na raiz do perfil).
    if !plan.cwd.join(".git").exists() || !plan.cwd.starts_with(&profile.root) {
        return Err(error(
            StatusCode::NOT_FOUND,
            "projeto não encontrado neste perfil",
        ));
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
    async fn pending_routes_say_which_feature() {
        let (status, body) = call(app(None), "/api/sleep", None).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["feature"], "F6.3");
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
