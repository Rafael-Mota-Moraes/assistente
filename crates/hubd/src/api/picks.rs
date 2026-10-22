//! Enviar ao agente (F12.4): o pick vira um pedido na sessão de um agente ou um card no
//! backlog do quadro.
//!
//! O pedido vai para a sessão colado no prompt, **sem Enter**: o Enter é do usuário, para não
//! aprovar uma pergunta pendente do agente nem executar texto no shell da aba Terminal. O
//! detalhe do pick fica em `.assistente/picks/<data-hora>.md`, dentro do projeto e fora do
//! git (como o `.ai-jail`, F0.5).

// Os handlers devolvem `Response` como o resto de `api.rs`; o `Err` grande é o preço disso.
#![allow(clippy::result_large_err)]

use super::*;
use crate::api::kanban::card_from_pick;
use crate::sessions::PaneState;
use axum::extract::Path as Params;
use hub_core::pick::{PICKS_DIR, PICKS_EXCLUDE, PickDetail, PickRequest, PickResult, PickTarget};
use std::path::{Path, PathBuf};

type Reply<T> = Result<T, Response>;

/// POST /api/projects/{perfil}/{projeto}/picks: manda o pedido apontado na tela.
pub(super) async fn send_pick(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
    Json(body): Json<PickRequest>,
) -> Reply<(StatusCode, Json<PickResult>)> {
    let dir = project_dir(&state, &profile, &project)?;
    body.validate()
        .map_err(|message| error(StatusCode::BAD_REQUEST, &message))?;
    // A sessão é conferida antes de escrever o arquivo: pedido recusado não deixa rastro.
    let session = match body.target {
        PickTarget::Sessao => Some(checked_session(&state, &profile, &project, &body).await?),
        PickTarget::Quadro => None,
    };
    let file = write_pick_file(&dir, &body).await?;
    let prompt = body.prompt();
    let mut result = PickResult {
        target: body.target,
        session: None,
        card: None,
        prompt: prompt.clone(),
        file: file.clone(),
    };
    match body.target {
        PickTarget::Sessao => {
            let id = session.expect("a sessão foi conferida antes");
            state
                .0
                .tmux
                .paste(&id, &prompt)
                .await
                .map_err(|e| internal(&format!("não consegui colar na sessão {id}: {e}")))?;
            result.session = Some(id);
        }
        PickTarget::Quadro => {
            let card = card_from_pick(
                &state,
                &profile,
                &project,
                &body.card_title(),
                &body.card_description(),
            )
            .await?;
            result.card = Some(card);
        }
    }
    Ok((StatusCode::CREATED, Json(result)))
}

/// Só o pick e o recado: o modal pergunta o prompt antes de enviar (F12.4).
#[derive(Deserialize)]
pub(super) struct PreviewBody {
    #[serde(default)]
    pick: PickDetail,
    #[serde(default)]
    request: String,
}

/// POST /api/projects/{perfil}/{projeto}/picks/preview: o prompt que o agente vai receber.
pub(super) async fn preview_pick(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
    Json(body): Json<PreviewBody>,
) -> Reply<Json<PickPreview>> {
    project_dir(&state, &profile, &project)?;
    let request = PickRequest {
        target: PickTarget::Sessao,
        session: None,
        request: body.request,
        pick: body.pick,
    };
    Ok(Json(PickPreview {
        prompt: request.prompt(),
    }))
}

/// O prompt que será enviado (F12.4).
#[derive(Serialize)]
pub(super) struct PickPreview {
    pub prompt: String,
}

/// A sessão que recebe o pedido: tem que ser de agente, do projeto e estar viva.
async fn checked_session(
    state: &AppState,
    profile: &str,
    project: &str,
    body: &PickRequest,
) -> Reply<String> {
    let id = body.session.as_deref().unwrap_or_default().trim();
    if id.is_empty() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "escolha a sessão que recebe o pedido",
        ));
    }
    let session = state
        .0
        .tmux
        .find(id)
        .await
        .map_err(|e| internal(&e))?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "sessão não encontrada"))?;
    if session.agent == AgentKind::Shell {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "a aba Terminal não recebe pedido: escolha uma sessão de agente",
        ));
    }
    if session.profile != profile || session.project != project {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "a sessão escolhida é de outro projeto",
        ));
    }
    // Com o `remain-on-exit` (F14.3) a sessão cujo agente saiu continua no tmux: colar ali
    // perderia o pedido em silêncio.
    if let PaneState::Dead(_) = state.0.tmux.pane_state(&session.id).await {
        return Err(error(
            StatusCode::CONFLICT,
            "a sessão escolhida terminou: abra outra ou mande para o quadro",
        ));
    }
    Ok(session.id)
}

/// Escreve `.assistente/picks/<data-hora>.md` com o detalhe do pick e devolve o caminho
/// (relativo à raiz do projeto, que é onde o agente está).
async fn write_pick_file(dir: &Path, body: &PickRequest) -> Reply<String> {
    // Fora do git: os picks são registro local, como o `.ai-jail` (F0.5).
    if let Err(e) = crate::sleep::exclude_line(dir, PICKS_EXCLUDE).await {
        tracing::warn!("sem exclude dos picks em {}: {e:#}", dir.display());
    }
    let picks = dir.join(PICKS_DIR);
    std::fs::create_dir_all(&picks)
        .map_err(|e| internal(&format!("não consegui criar {}: {e}", picks.display())))?;
    let stamp = stamp().await?;
    let mut name = format!("{stamp}.md");
    let mut n = 2;
    while picks.join(&name).exists() {
        name = format!("{stamp}-{n}.md");
        n += 1;
    }
    let path = picks.join(&name);
    std::fs::write(&path, body.markdown(&stamp))
        .map_err(|e| internal(&format!("não consegui escrever {}: {e}", path.display())))?;
    Ok(format!("{PICKS_DIR}/{name}"))
}

/// `2026-10-22-11-44-05`: a data e a hora do pick (nome do arquivo).
async fn stamp() -> Reply<String> {
    let out = tokio::process::Command::new("date")
        .arg("+%Y-%m-%d-%H-%M-%S")
        .output()
        .await
        .map_err(|e| internal(&format!("não consegui ler a data: {e}")))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    // O nome vai para um caminho: só dígitos e traços.
    if text.is_empty() || !text.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return Err(internal("não consegui ler a data"));
    }
    Ok(text)
}

/// A pasta do projeto (mesma regra das outras rotas: repo git na raiz do perfil).
fn project_dir(state: &AppState, profile: &str, project: &str) -> Reply<PathBuf> {
    sleep_project_dir(state, profile, project).map_err(|(status, message)| error(status, message))
}

#[cfg(test)]
mod tests {
    use super::super::{AppState, router};
    use super::*;
    use crate::sessions::tests::TestTmux;
    use crate::sleep::tests::{git_sync, init_repo};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use hub_core::AgentKind;
    use serde_json::json;
    use tower::ServiceExt;

    const EXAMPLE: &str = include_str!("../../../../config/assistente.exemplo.toml");
    const PICKS: &str = "/api/projects/pessoal/proj/picks";

    struct Hub {
        app: Router,
        /// Repo `proj` de verdade (git), perfil `pessoal`.
        proj: PathBuf,
        /// Onde o `cat` da sessão de teste espera o que chega.
        received: PathBuf,
    }

    /// hubd de verdade (tmux de verdade) com o repo `proj` e o `outro` ao lado.
    fn hub(t: &TestTmux) -> Hub {
        let root = t.dir.join("raiz");
        let proj = root.join("proj");
        init_repo(&proj);
        init_repo(&root.join("outro"));
        let home = t.dir.join("home");
        let text = EXAMPLE.replace("~/Documentos/pessoal/projetos", &root.display().to_string());
        let config = Config::parse(&text, &home).unwrap();
        let state = AppState::new(config, home, None, None, t.tmux.clone())
            .with_argv_override(vec!["bash".into(), "--norc".into(), "-i".into()]);
        Hub {
            app: router(state),
            proj,
            received: t.dir.join("recebido.txt"),
        }
    }

    async fn send(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_owned()));
        let response = app.oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    /// O corpo do pedido: o pick apontado e o recado.
    fn body(target: &str, session: Option<&str>, request: &str) -> String {
        json!({
            "target": target,
            "session": session,
            "request": request,
            "pick": {
                "url": "http://localhost:5173",
                "selector": "button.salvar",
                "text": "Salvar",
                "html": "<button class=\"salvar\">Salvar</button>",
                "tag": "button",
                "component": "Botao",
                "source": "src/features/x/Botao.tsx:42"
            }
        })
        .to_string()
    }

    /// Sessão de agente de verdade, rodando o `cat` que espera o que é colado.
    async fn agent_session(t: &TestTmux, hub: &Hub, project: &str) -> String {
        let id = format!("pessoal--{project}--claude-1");
        let script = format!("cat > {}", hub.received.display());
        let spec = hub_core::SessionSpec {
            profile: "pessoal".into(),
            project: project.into(),
            agent: AgentKind::Claude,
        };
        t.tmux
            .create(&t.plan(&id, &script), &spec, None)
            .await
            .unwrap();
        id
    }

    /// O que o tmux mostra no painel da sessão.
    async fn pane(t: &TestTmux, session: &str) -> String {
        let out = t
            .tmux
            .command(&[
                "capture-pane".into(),
                "-p".into(),
                "-t".into(),
                session.into(),
            ])
            .output()
            .await
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    async fn press_enter(t: &TestTmux, session: &str) {
        t.tmux
            .command(&[
                "send-keys".into(),
                "-t".into(),
                session.into(),
                "Enter".into(),
            ])
            .output()
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn pick_goes_to_the_agent_session_without_pressing_enter() {
        let t = TestTmux::new("picks-sessao");
        let hub = hub(&t);
        let session = agent_session(&t, &hub, "proj").await;

        let (status, answer) = send(
            hub.app.clone(),
            "POST",
            PICKS,
            Some(&body("sessao", Some(&session), "deixa verde")),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        assert_eq!(answer["target"], "sessao");
        assert_eq!(answer["session"], session.as_str());
        let prompt = answer["prompt"].as_str().unwrap();
        assert!(prompt.contains("deixa verde"), "{prompt}");
        assert!(prompt.contains("button.salvar"), "{prompt}");
        assert!(prompt.contains("src/features/x/Botao.tsx:42"), "{prompt}");

        // O texto colado está no prompt do agente, mas o Enter é do usuário: o `cat` não
        // recebeu nada ainda.
        assert!(pane(&t, &session).await.contains("deixa verde"));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let received = std::fs::read_to_string(&hub.received).unwrap_or_default();
        assert!(received.is_empty(), "o pedido rodou sozinho: {received}");

        // Com o Enter do usuário, o texto chega.
        press_enter(&t, &session).await;
        let mut received = String::new();
        for _ in 0..50 {
            received = std::fs::read_to_string(&hub.received).unwrap_or_default();
            if !received.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(received.contains("deixa verde"), "{received}");
    }

    #[tokio::test]
    async fn pick_writes_the_detail_in_the_project_out_of_the_git() {
        let t = TestTmux::new("picks-arquivo");
        let hub = hub(&t);
        let session = agent_session(&t, &hub, "proj").await;

        let (status, answer) = send(
            hub.app.clone(),
            "POST",
            PICKS,
            Some(&body("sessao", Some(&session), "deixa verde")),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        let file = answer["file"].as_str().unwrap().to_owned();
        assert!(file.starts_with(".assistente/picks/"), "{file}");
        let text = std::fs::read_to_string(hub.proj.join(&file)).unwrap();
        assert!(text.contains("src/features/x/Botao.tsx:42"), "{text}");
        assert!(text.contains("deixa verde"), "{text}");

        // Fora do git: o `info/exclude` do repo e a árvore limpa.
        let exclude = std::fs::read_to_string(hub.proj.join(".git/info/exclude")).unwrap();
        assert!(exclude.contains(PICKS_EXCLUDE), "{exclude}");
        assert_eq!(git_sync(&hub.proj, &["status", "--porcelain"]), "");
    }

    #[tokio::test]
    async fn pick_to_the_board_becomes_a_card_that_stands_alone() {
        let t = TestTmux::new("picks-quadro");
        let hub = hub(&t);

        let (status, answer) = send(
            hub.app.clone(),
            "POST",
            PICKS,
            Some(&body("quadro", None, "deixa verde")),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{answer}");
        assert_eq!(answer["target"], "quadro");
        let card = &answer["card"];
        assert_eq!(card["id"], "T1");
        assert_eq!(card["column"], "backlog");
        assert_eq!(card["title"], "deixa verde");
        let description = card["description"].as_str().unwrap();
        assert!(
            description.contains("URL: http://localhost:5173"),
            "{description}"
        );
        assert!(
            description.contains("Onde: src/features/x/Botao.tsx:42"),
            "{description}"
        );
        assert!(
            description.contains("Seletor: button.salvar"),
            "{description}"
        );
        assert!(description.contains("Texto: Salvar"), "{description}");
        assert!(description.contains("Pedido: deixa verde"), "{description}");
        // O clone do notebook não tem o arquivo do pick: o card não depende dele.
        assert!(!description.contains(".assistente/picks"), "{description}");

        // O card está no quadro do projeto.
        let (_, board) = send(
            hub.app.clone(),
            "GET",
            "/api/projects/pessoal/proj/kanban",
            None,
        )
        .await;
        assert_eq!(board["cards"][0]["id"], "T1");
    }

    #[tokio::test]
    async fn pick_guards_the_text_the_session_and_the_project() {
        let t = TestTmux::new("picks-guardas");
        let hub = hub(&t);
        let session = agent_session(&t, &hub, "proj").await;

        // Texto vazio.
        let (status, answer) = send(
            hub.app.clone(),
            "POST",
            PICKS,
            Some(&body("sessao", Some(&session), "   ")),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert!(
            answer["error"].as_str().unwrap().contains("vazio"),
            "{answer}"
        );

        // Sessão que não existe.
        let (status, _) = send(
            hub.app.clone(),
            "POST",
            PICKS,
            Some(&body(
                "sessao",
                Some("pessoal--proj--claude-9"),
                "deixa verde",
            )),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Sessão de outro projeto.
        let outra = agent_session(&t, &hub, "outro").await;
        let (status, answer) = send(
            hub.app.clone(),
            "POST",
            PICKS,
            Some(&body("sessao", Some(&outra), "deixa verde")),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert!(
            answer["error"].as_str().unwrap().contains("outro projeto"),
            "{answer}"
        );

        // A aba Terminal (fora do jail) não recebe pedido.
        let shell = "pessoal--proj--shell-1";
        let spec = hub_core::SessionSpec {
            profile: "pessoal".into(),
            project: "proj".into(),
            agent: AgentKind::Shell,
        };
        t.tmux
            .create(&t.plan(shell, "sleep 300"), &spec, None)
            .await
            .unwrap();
        let (status, answer) = send(
            hub.app.clone(),
            "POST",
            PICKS,
            Some(&body("sessao", Some(shell), "deixa verde")),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
        assert!(
            answer["error"].as_str().unwrap().contains("Terminal"),
            "{answer}"
        );

        // Sessão cujo agente já saiu: o painel fica (F14.3), mas não recebe pedido.
        let morta = "pessoal--proj--claude-2";
        let spec = hub_core::SessionSpec {
            profile: "pessoal".into(),
            project: "proj".into(),
            agent: AgentKind::Claude,
        };
        t.tmux
            .create(&t.plan(morta, "exit 1"), &spec, None)
            .await
            .unwrap();
        let (status, answer) = send(
            hub.app.clone(),
            "POST",
            PICKS,
            Some(&body("sessao", Some(morta), "deixa verde")),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{answer}");
        assert!(
            answer["error"].as_str().unwrap().contains("terminou"),
            "{answer}"
        );

        // Pedido recusado não deixa rastro no projeto.
        assert!(!hub.proj.join(PICKS_DIR).exists());

        // Projeto que não existe.
        let (status, _) = send(
            hub.app.clone(),
            "POST",
            "/api/projects/pessoal/nao-existe/picks",
            Some(&body("quadro", None, "deixa verde")),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn preview_shows_the_prompt_that_will_be_sent() {
        let t = TestTmux::new("picks-preview");
        let hub = hub(&t);
        let (status, answer) = send(
            hub.app.clone(),
            "POST",
            "/api/projects/pessoal/proj/picks/preview",
            Some(
                &json!({
                    "request": "deixa verde",
                    "pick": {"url": "http://localhost:5173", "selector": "button.salvar"}
                })
                .to_string(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{answer}");
        let prompt = answer["prompt"].as_str().unwrap();
        assert!(prompt.contains("deixa verde"), "{prompt}");
        assert!(prompt.contains("button.salvar"), "{prompt}");
        assert!(!prompt.contains('\n'), "{prompt}");
    }
}
