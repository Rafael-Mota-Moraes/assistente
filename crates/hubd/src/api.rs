//! Rotas HTTP do hubd. Só `/api/health` é aberta; o resto exige `Authorization: Bearer <token>`.
//! Rotas ainda não implementadas respondem 501 com o ID da funcionalidade (docs/FUNCIONALIDADES.md).

use axum::extract::{Query, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hub_core::{Config, Project, discover_projects};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    config: Config,
    #[allow(dead_code)] // usado a partir da F2 (montagem das sessões)
    home: PathBuf,
    token: Option<String>,
    started: Instant,
}

impl AppState {
    pub fn new(config: Config, home: PathBuf, token: Option<String>) -> Self {
        Self(Arc::new(Inner {
            config,
            home,
            token,
            started: Instant::now(),
        }))
    }
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/profiles", get(profiles))
        .route("/api/projects", get(projects))
        .route("/api/notebook", get(notebook))
        .route(
            "/api/sessions",
            get(|| not_yet("F2.2")).post(|| not_yet("F2.2")),
        )
        .route(
            "/api/sessions/{id}",
            axum::routing::delete(|| not_yet("F2.2")),
        )
        .route("/api/sessions/{id}/terminal", get(|| not_yet("F2.3")))
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
    let given = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
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
    /// O túnel SSH ainda não é gerenciado pelo hubd (F4.2); hoje só testamos as portas locais.
    tunnel_managed: bool,
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
        tunnel_managed: false,
        services,
    })
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
        router(AppState::new(config, home, token.map(str::to_owned)))
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
        let (status, body) = call(app(None), "/api/sessions", None).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["feature"], "F2.2");
    }
}
