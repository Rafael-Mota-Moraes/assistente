//! Quadro dos projetos e trabalhadores (F11). Da UI: `/api/projects/{perfil}/{projeto}/kanban…`,
//! `…/kanban/{id}/mandar` e `/api/sessions/{id}/reiniciar`. Da API dos agentes (token da sessão):
//! `/v1/kanban/{pull,done,release}` e `/v1/workers`, que só o orquestrador (sessão do Claude) usa.
//! O orquestrador automático (F11.11, `spawn_orchestrator`) é um laço do próprio hubd: liga em
//! `PUT …/kanban/orquestrador` e abre trabalhadores para as tarefas `agente`, sem sessão do Claude.
//! Regras em `hub_core::kanban`; arquivo e commit em `crate::kanban`; trabalhador em `hub_core::worker`.

// Os handlers devolvem `Response` como o resto de `api.rs`; o `Err` grande é o preço disso.
#![allow(clippy::result_large_err)]

use super::*;
use crate::kanban as store;
use axum::extract::Path as Params;
use hub_core::kanban::{Board, Card, Column, Draft, KanbanError};
use hub_core::worker::{self, WorkerRun};
use hub_core::{profile_data_dir, workers_config_at};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

type Reply<T> = Result<T, Response>;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn project_dir(state: &AppState, profile: &str, project: &str) -> Reply<PathBuf> {
    sleep_project_dir(state, profile, project).map_err(|(status, message)| error(status, message))
}

fn kanban_error(e: KanbanError) -> Response {
    match e {
        KanbanError::NotFound => error(StatusCode::NOT_FOUND, "tarefa não encontrada"),
        KanbanError::Invalid(message) => error(StatusCode::BAD_REQUEST, &message),
        other => error(StatusCode::CONFLICT, &other.to_string()),
    }
}

fn column_name(column: Column) -> &'static str {
    match column {
        Column::Backlog => "backlog",
        Column::Fazendo => "fazendo",
        Column::Feito => "feito",
    }
}

/// Quadro atual. Tarefa presa a sessão que já não existe volta ao backlog (F11.1) e o
/// commit fica registrado. Sessão morta conta como encerrada: com o `remain-on-exit` (F14.3)
/// a sessão continua no tmux depois que o processo morre, mas não segura tarefa nenhuma.
/// Sem a lista de sessões (o tmux em transição, p. ex. ao encerrar a última sessão), nada é
/// solto: o quadro lido segue. Chamar sempre com `kanban_lock` segurado.
async fn load(state: &AppState, dir: &Path) -> Reply<Board> {
    let mut board = store::read(dir).map_err(|e| internal(&e))?;
    match state.0.tmux.list_running().await {
        Ok(sessions) => {
            let alive: HashSet<String> = sessions.into_iter().map(|session| session.id).collect();
            let released = board.release_dead(|holder| alive.contains(holder));
            if !released.is_empty() {
                let message = format!("kanban: solta {} (sessão encerrada)", released.join(", "));
                store::save(dir, &board, &message).map_err(|e| internal(&e))?;
            }
        }
        Err(e) => tracing::warn!("quadro: sem a lista de sessões, nada foi solto: {e}"),
    }
    Ok(board)
}

fn persist(dir: &Path, board: &Board, message: &str) -> Reply<()> {
    store::save(dir, board, message).map_err(|e| internal(&e))
}

/// Commit informado pelo trabalhador: só hexadecimal, para não virar texto livre no quadro.
fn checked_commit(commit: Option<String>) -> Reply<Option<String>> {
    let Some(commit) = commit.map(|c| c.trim().to_owned()) else {
        return Ok(None);
    };
    if (4..=64).contains(&commit.len()) && commit.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(Some(commit))
    } else {
        Err(error(StatusCode::BAD_REQUEST, "commit inválido"))
    }
}

/// Quadro para a UI: o arquivo mais o último erro do orquestrador automático (só em memória).
#[derive(Serialize)]
pub(super) struct BoardView {
    #[serde(flatten)]
    board: Board,
    #[serde(skip_serializing_if = "Option::is_none")]
    orchestrator_error: Option<String>,
    /// Só na resposta do `PUT …/orquestrador`: tarefas que a volta abriu agora, para o aviso.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    started: Vec<String>,
}

fn board_view(state: &AppState, profile: &str, project: &str, board: Board) -> BoardView {
    let errors = state.0.orchestrator_errors.lock().expect("lock dos erros");
    BoardView {
        orchestrator_error: errors.get(&orchestrator_key(profile, project)).cloned(),
        board,
        started: Vec::new(),
    }
}

/// GET /api/projects/{perfil}/{projeto}/kanban
pub(super) async fn list_cards(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
) -> Reply<Json<BoardView>> {
    let dir = project_dir(&state, &profile, &project)?;
    let board = {
        let _guard = state.0.kanban_lock.lock().await;
        load(&state, &dir).await?
    };
    Ok(Json(board_view(&state, &profile, &project, board)))
}

#[derive(Deserialize)]
pub(super) struct OrchestratorBody {
    enabled: bool,
    #[serde(default = "one")]
    max_workers: u32,
    /// Ausente = mantém as instruções atuais.
    #[serde(default)]
    instructions: Option<String>,
}

fn one() -> u32 {
    1
}

/// PUT /api/projects/{perfil}/{projeto}/kanban/orquestrador `{enabled, max_workers,
/// instructions?}`: liga ou desliga o orquestrador automático (F11.11) e guarda as instruções dos
/// trabalhadores. Ligado, já dá a primeira volta antes de responder (`started`).
pub(super) async fn set_orchestrator(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
    Json(body): Json<OrchestratorBody>,
) -> Reply<Json<BoardView>> {
    let dir = project_dir(&state, &profile, &project)?;
    {
        let _guard = state.0.kanban_lock.lock().await;
        let mut board = load(&state, &dir).await?;
        let before = board.orchestrator.clone();
        let set = board
            .set_orchestrator(body.enabled, body.max_workers, body.instructions)
            .map_err(kanban_error)?
            .clone();
        let message = if set.enabled && (!before.enabled || set.max_workers != before.max_workers) {
            format!(
                "kanban: orquestrador ligado (até {} em fazendo)",
                set.max_workers
            )
        } else if before.enabled && !set.enabled {
            "kanban: orquestrador desligado".to_owned()
        } else {
            "kanban: instruções dos trabalhadores".to_owned()
        };
        persist(&dir, &board, &message)?;
    }
    let started = orchestrate_project(&state, &profile, &project).await;
    let board = {
        let _guard = state.0.kanban_lock.lock().await;
        load(&state, &dir).await?
    };
    let mut view = board_view(&state, &profile, &project, board);
    view.started = started;
    Ok(Json(view))
}

/// POST /api/projects/{perfil}/{projeto}/kanban: card novo nasce no backlog.
pub(super) async fn create_card(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
    Json(mut draft): Json<Draft>,
) -> Reply<(StatusCode, Json<Card>)> {
    let dir = project_dir(&state, &profile, &project)?;
    draft.column = None;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board.create(draft, now()).map_err(kanban_error)?.clone();
    persist(&dir, &board, &format!("kanban: cria {}", card.id))?;
    Ok((StatusCode::CREATED, Json(card)))
}

/// Card novo no backlog a partir de um pedido apontado na tela (F12.4). O texto é
/// autossuficiente: o clone do notebook não tem o arquivo do pick.
pub(super) async fn card_from_pick(
    state: &AppState,
    profile: &str,
    project: &str,
    title: &str,
    description: &str,
) -> Reply<Card> {
    let dir = project_dir(state, profile, project)?;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(state, &dir).await?;
    let draft = Draft {
        title: Some(title.to_owned()),
        description: Some(description.to_owned()),
        ..Default::default()
    };
    let card = board.create(draft, now()).map_err(kanban_error)?.clone();
    persist(&dir, &board, &format!("kanban: cria {}", card.id))?;
    Ok(card)
}

/// PATCH /api/projects/{perfil}/{projeto}/kanban/{id}: edita, ou move de coluna.
pub(super) async fn update_card(
    State(state): State<AppState>,
    Params((profile, project, id)): Params<(String, String, String)>,
    Json(draft): Json<Draft>,
) -> Reply<Json<Card>> {
    let dir = project_dir(&state, &profile, &project)?;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let before = board
        .get(&id)
        .map(|card| card.column)
        .ok_or_else(|| kanban_error(KanbanError::NotFound))?;
    let card = board.update(&id, draft).map_err(kanban_error)?.clone();
    let message = if card.column != before {
        format!("kanban: move {id} para {}", column_name(card.column))
    } else {
        format!("kanban: edita {id}")
    };
    persist(&dir, &board, &message)?;
    Ok(Json(card))
}

/// DELETE /api/projects/{perfil}/{projeto}/kanban/{id}
pub(super) async fn delete_card(
    State(state): State<AppState>,
    Params((profile, project, id)): Params<(String, String, String)>,
) -> Reply<StatusCode> {
    let dir = project_dir(&state, &profile, &project)?;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    board.delete(&id).map_err(kanban_error)?;
    persist(&dir, &board, &format!("kanban: apaga {id}"))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct StartBody {
    /// Principal e reservas, nessa ordem. Vazio = os de `[workers]` do projeto.
    models: Vec<String>,
}

/// POST /api/projects/{perfil}/{projeto}/kanban/{id}/mandar: você manda uma tarefa `manual`
/// para um trabalhador. É o único jeito de ela rodar (F11.6).
pub(super) async fn mandar_card(
    State(state): State<AppState>,
    Params((profile, project, id)): Params<(String, String, String)>,
    body: Option<Json<StartBody>>,
) -> Reply<(StatusCode, Json<SessionInfo>)> {
    let models = body.map(|Json(b)| b.models).unwrap_or_default();
    let info = start_worker(&state, &profile, &project, &id, models, None).await?;
    Ok((StatusCode::CREATED, Json(info)))
}

/// POST /api/sessions/{id}/reiniciar: relança um trabalhador (o botão "reiniciar" da UI). Mata
/// a sessão antiga e abre outra com o mesmo modelo, no mesmo worktree; a tarefa passa para ela.
pub(super) async fn restart_worker(
    State(state): State<AppState>,
    Params(session_id): Params<String>,
) -> Reply<(StatusCode, Json<SessionInfo>)> {
    let inner = &state.0;
    let old = inner
        .tmux
        .find(&session_id)
        .await
        .map_err(|e| internal(&e))?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "sessão não encontrada"))?;
    let files = worker::files(&profile_data_dir(&inner.home, &old.profile), &old.id);
    let run: WorkerRun = std::fs::read_to_string(&files.run)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "esta sessão não é um trabalhador"))?;
    let dir = project_dir(&state, &old.profile, &old.project)?;
    let _guard = inner.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board
        .get(&run.task)
        .cloned()
        .ok_or_else(|| kanban_error(KanbanError::NotFound))?;
    // A sessão antiga sai antes: dois trabalhadores no mesmo worktree se atrapalham.
    inner.tmux.kill(&old.id).await.map_err(|e| internal(&e))?;
    let branch = worker::branch(&run.task);
    let info = spawn_worker(
        &state,
        &old.profile,
        &old.project,
        &run,
        &worker::render_prompt(&card, &branch, &board.orchestrator.instructions),
    )
    .await?;
    let holds_it = card.holder.as_deref() == Some(old.id.as_str());
    let moved = if holds_it {
        board.hand_over(&run.task, &old.id, &info.id).map(|_| ())
    } else {
        board.claim(&run.task, &info.id).map(|_| ())
    };
    let message = format!(
        "kanban: {} reiniciada com trabalhador {}",
        run.task, info.id
    );
    let saved = moved
        .map_err(kanban_error)
        .and_then(|_| persist(&dir, &board, &message));
    if let Err(response) = saved {
        let _ = inner.tmux.kill(&info.id).await;
        return Err(response);
    }
    Ok((StatusCode::CREATED, Json(info)))
}

/// Abre o trabalhador da tarefa `id`. `holder` = a sessão que já segura a tarefa (o orquestrador
/// passa a tarefa dele ao trabalhador); `None` = a tarefa precisa estar no backlog (Mandar).
async fn start_worker(
    state: &AppState,
    profile: &str,
    project: &str,
    id: &str,
    models: Vec<String>,
    holder: Option<&str>,
) -> Reply<SessionInfo> {
    let dir = project_dir(state, profile, project)?;
    let config = workers_config_at(&dir);
    // Pedido > `[workers] models` do projeto > escolha automática entre os grátis (F11.10).
    let models = if !models.is_empty() {
        models
    } else if !config.models.is_empty() {
        config.models.clone()
    } else {
        auto_models(state, profile).await?
    };
    let models = worker::checked_models(&models).map_err(|e| error(StatusCode::BAD_REQUEST, &e))?;
    // O id vai para um caminho de pasta e um nome de branch: só o formato gerado pelo quadro.
    if !worker::valid_task_id(id) {
        return Err(error(StatusCode::BAD_REQUEST, "id de tarefa inválido"));
    }
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(state, &dir).await?;
    let card = board
        .get(id)
        .cloned()
        .ok_or_else(|| kanban_error(KanbanError::NotFound))?;
    match holder {
        Some(_) if card.column != Column::Fazendo => {
            return Err(kanban_error(KanbanError::NotInProgress(card.column)));
        }
        Some(holder) if card.holder.as_deref() != Some(holder) => {
            return Err(kanban_error(KanbanError::NotHolder));
        }
        None if card.column != Column::Backlog => {
            return Err(kanban_error(KanbanError::NotInBacklog(card.column)));
        }
        _ => {}
    }
    let branch = worker::branch(id);
    let worktree = worker::worktree_dir(&dir, id);
    ensure_worktree(&dir, &worktree, &branch).await?;
    let run = WorkerRun {
        task: id.to_owned(),
        models,
        attempts: config.attempts,
        max_minutes: config.max_minutes,
        wait_secs: config.wait_secs,
        worktree,
    };
    let info = spawn_worker(
        state,
        profile,
        project,
        &run,
        &worker::render_prompt(&card, &branch, &board.orchestrator.instructions),
    )
    .await?;
    let moved = match holder {
        Some(holder) => board.hand_over(id, holder, &info.id).map(|_| ()),
        None => board.claim(id, &info.id).map(|_| ()),
    };
    let message = format!("kanban: {id} com trabalhador {}", info.id);
    let saved = moved
        .map_err(kanban_error)
        .and_then(|_| persist(&dir, &board, &message));
    if let Err(response) = saved {
        let _ = state.0.tmux.kill(&info.id).await;
        return Err(response);
    }
    Ok(info)
}

/// Modelos grátis do opencode, lidos do catálogo que ele mantém (F11.10), na ordem do histórico
/// de sucesso do perfil. Sem catálogo ou sem nenhum grátis, responde erro com o que fazer.
async fn auto_models(state: &AppState, profile: &str) -> Reply<Vec<String>> {
    let inner = &state.0;
    let data = profile_data_dir(&inner.home, profile);
    let path = worker::catalogue_path(&inner.home);
    let catalogue = std::fs::read_to_string(&path).map_err(|e| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            &format!(
                "catálogo do opencode não lido em {} ({e}): abra o opencode uma vez",
                path.display()
            ),
        )
    })?;
    let free = worker::free_models(&catalogue);
    if free.is_empty() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "nenhum modelo grátis do opencode no catálogo: escolha um modelo ou defina [workers] models",
        ));
    }
    let ranked = worker::rank_models(&free, &worker::read_outcomes(&data));
    Ok(ranked.into_iter().take(worker::MAX_MODELS).collect())
}

/// Worktree da tarefa (`.assistente/worktrees/<id>`, branch `agente/<id>`). Reaproveita o que já
/// existe: é assim que um reinício continua do que o trabalhador deixou commitado.
async fn ensure_worktree(dir: &Path, worktree: &Path, branch: &str) -> Reply<()> {
    if worktree.join(".git").exists() {
        return Ok(());
    }
    if let Some(parent) = worktree.parent() {
        std::fs::create_dir_all(parent).map_err(|e| internal(&e.to_string()))?;
    }
    let path = worktree.display().to_string();
    let has_branch = git_out(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok();
    let added = if has_branch {
        git_out(dir, &["worktree", "add", &path, branch])
    } else {
        git_out(dir, &["worktree", "add", "-b", branch, &path, "HEAD"])
    };
    added.map_err(|e| internal(&e))?;
    crate::sleep::exclude_line(dir, hub_core::worker::WORKTREES_EXCLUDE)
        .await
        .map_err(|e| internal(&format!("{e:#}")))
}

/// Cria a sessão do trabalhador (opencode no jail, rodando `vm/trabalhador` no worktree) e
/// grava o prompt e o registro da execução, que o `trabalhador` e o reinício leem.
async fn spawn_worker(
    state: &AppState,
    profile: &str,
    project: &str,
    run: &WorkerRun,
    prompt: &str,
) -> Reply<SessionInfo> {
    let inner = &state.0;
    let spec = SessionSpec {
        profile: profile.to_owned(),
        project: project.to_owned(),
        agent: AgentKind::Opencode,
    };
    let existing = inner.tmux.list().await.map_err(|e| internal(&e))?;
    let seq = next_seq(&existing, &spec);
    // Sem a API dos agentes o trabalhador não consegue marcar a tarefa: não abre.
    let exec = exec_access_for(inner, AgentKind::Opencode).ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "a API dos agentes está desligada: o trabalhador não consegue marcar a tarefa",
        )
    })?;
    let token = exec.token.clone();
    let options = LaunchOptions {
        settings: project_settings_for(inner, profile, project),
        exec: Some(exec),
    };
    let mut plan = LaunchPlan::build_worker(&inner.config, &inner.home, &spec, seq, &options, run)
        .map_err(|e| error(StatusCode::BAD_REQUEST, &e.to_string()))?;
    if let Err(e) = sleep::exclude_jail_config(&run.worktree).await {
        tracing::warn!(
            "sem exclude do .ai-jail em {}: {e:#}",
            run.worktree.display()
        );
    }
    let files = worker::files(&profile_data_dir(&inner.home, profile), &plan.tmux_session);
    write_worker_files(&files, prompt, run)?;
    if let Some(argv) = &inner.argv_override {
        plan.argv = argv.clone();
    }
    let info = inner
        .tmux
        .create(&plan, &spec, Some(&token))
        .await
        .map_err(|e| internal(&e))?;
    tracing::info!("trabalhador {} na tarefa {}", info.id, run.task);
    Ok(info)
}

fn write_worker_files(files: &worker::WorkerFiles, prompt: &str, run: &WorkerRun) -> Reply<()> {
    if let Some(parent) = files.prompt.parent() {
        std::fs::create_dir_all(parent).map_err(|e| internal(&e.to_string()))?;
    }
    std::fs::write(&files.prompt, prompt).map_err(|e| internal(&e.to_string()))?;
    let run_json = serde_json::to_string_pretty(run).map_err(|e| internal(&e.to_string()))?;
    std::fs::write(&files.run, run_json).map_err(|e| internal(&e.to_string()))
}

/// POST /api/projects/{perfil}/{projeto}/kanban/explorar: abre um Claude que lê o backlog em
/// markdown e monta o quadro (F11.9). Os cards nascem `manual`; você marca os que vão para o agente.
pub(super) async fn explore_backlog(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
) -> Reply<(StatusCode, Json<SessionInfo>)> {
    project_dir(&state, &profile, &project)?;
    let spec = SessionSpec {
        profile,
        project,
        agent: AgentKind::Claude,
    };
    let info = super::launch_session(&state, &spec, Some(worker::EXPLORE_PROMPT)).await?;
    Ok((StatusCode::CREATED, Json(info)))
}

/// GET /v1/kanban (API dos agentes): o quadro inteiro, para não criar card repetido.
pub(super) async fn agent_board(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Reply<Json<Board>> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let _guard = state.0.kanban_lock.lock().await;
    Ok(Json(load(&state, &dir).await?))
}

/// POST /v1/kanban/criar `{title, description?, priority?, kind?}` (API dos agentes): card novo no
/// backlog. Sem `kind`, nasce `manual`.
pub(super) async fn agent_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(draft): Json<Draft>,
) -> Reply<(StatusCode, Json<Card>)> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let mut draft = draft;
    draft.column = None;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board.create(draft, now()).map_err(kanban_error)?.clone();
    persist(
        &dir,
        &board,
        &format!("kanban: cria {} (por {})", card.id, session.id),
    )?;
    Ok((StatusCode::CREATED, Json(card)))
}

#[derive(Deserialize)]
pub(super) struct DoneBody {
    id: String,
    #[serde(default)]
    commit: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct TaskBody {
    id: String,
}

/// POST /v1/kanban/pull: a primeira tarefa `agente` do backlog, presa a esta sessão. Sem tarefa,
/// `{"card": null}`: o agente fica parado.
pub(super) async fn agent_pull(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Reply<Json<serde_json::Value>> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board.pull(&session.id).cloned();
    if let Some(card) = &card {
        let message = format!("kanban: {} puxada por {}", card.id, session.id);
        persist(&dir, &board, &message)?;
    }
    Ok(Json(json!({ "card": card })))
}

/// POST /v1/kanban/done `{id, commit?}`: conclui uma tarefa que esta sessão segura.
pub(super) async fn agent_done(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DoneBody>,
) -> Reply<Json<Card>> {
    let session = agent_session(&state, &headers).await?;
    let commit = checked_commit(body.commit)?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board
        .done(&body.id, &session.id, commit)
        .map_err(kanban_error)?
        .clone();
    persist(&dir, &board, &format!("kanban: {} concluída", card.id))?;
    Ok(Json(card))
}

/// POST /v1/kanban/release `{id}`: devolve ao backlog uma tarefa que esta sessão segura.
///
/// F14.4: a sessão que desiste fica com o painel na tela (o `remain-on-exit` da janela dela
/// é ligado aqui). É o trabalhador que desiste: ele sai logo depois (código 1) e, sem isto,
/// a sessão morria junto com ele e o erro sumia.
pub(super) async fn agent_release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<TaskBody>,
) -> Reply<Json<Card>> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board
        .release(&body.id, &session.id)
        .map_err(kanban_error)?
        .clone();
    persist(
        &dir,
        &board,
        &format!("kanban: {} devolvida ao backlog", card.id),
    )?;
    // A sessão que desiste guarda o painel: quem chamou está a caminho de morrer e o erro
    // precisa ficar visível na aba.
    if let Err(e) = state.0.tmux.keep_panel(&session.id).await {
        tracing::warn!("sessão {}: não consegui manter o painel: {e}", session.id);
    }
    Ok(Json(card))
}

#[derive(Deserialize)]
pub(super) struct WorkerBody {
    task: String,
    #[serde(default)]
    models: Vec<String>,
}

/// POST /v1/workers `{task, models}`: o orquestrador (sessão do Claude) abre um trabalhador para
/// uma tarefa que ele mesmo puxou. Os trabalhadores não abrem outros.
pub(super) async fn agent_worker(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<WorkerBody>,
) -> Reply<(StatusCode, Json<SessionInfo>)> {
    let session = agent_session(&state, &headers).await?;
    if session.agent != AgentKind::Claude {
        return Err(error(
            StatusCode::FORBIDDEN,
            "só o orquestrador (sessão do Claude) abre trabalhadores",
        ));
    }
    let info = start_worker(
        &state,
        &session.profile,
        &session.project,
        &body.task,
        body.models,
        Some(&session.id),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(info)))
}

/// A cada quanto o orquestrador automático olha os quadros (F11.11).
const ORCHESTRATOR_EVERY: Duration = Duration::from_secs(20);

fn orchestrator_key(profile: &str, project: &str) -> String {
    format!("{profile}/{project}")
}

/// Laço do orquestrador automático (F11.11): a cada `ORCHESTRATOR_EVERY`, cada projeto com o
/// orquestrador ligado no quadro ganha trabalhadores para as próximas tarefas `agente`, até o
/// limite. Roda enquanto o hubd roda; desligar no quadro basta para ele parar de abrir.
pub fn spawn_orchestrator(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(ORCHESTRATOR_EVERY);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            timer.tick().await;
            orchestrate_all(&state).await;
        }
    })
}

async fn orchestrate_all(state: &AppState) {
    for (profile, config) in &state.0.config.profiles {
        let root = config.root.clone();
        let projects = match tokio::task::spawn_blocking(move || discover_projects(&root)).await {
            Ok(Ok(projects)) => projects,
            Ok(Err(e)) => {
                tracing::warn!("orquestrador: não listei os projetos de {profile}: {e}");
                continue;
            }
            Err(_) => continue,
        };
        for project in projects {
            // Leitura sem lock: só decide se vale a volta; a volta relê o quadro sob o lock.
            let enabled = store::read(&project.path).is_ok_and(|b| b.orchestrator.enabled);
            if enabled {
                orchestrate_project(state, profile, &project.name).await;
            }
        }
    }
}

/// Uma volta num projeto, com o erro guardado para o quadro mostrar (e o log só quando muda).
/// Devolve as tarefas que começaram.
async fn orchestrate_project(state: &AppState, profile: &str, project: &str) -> Vec<String> {
    let result = orchestrate(state, profile, project).await;
    let key = orchestrator_key(profile, project);
    let mut errors = state.0.orchestrator_errors.lock().expect("lock dos erros");
    match result {
        Ok(started) => {
            for id in &started {
                tracing::info!("orquestrador: {key} {id} com trabalhador");
            }
            errors.remove(&key);
            started
        }
        Err(e) => {
            if errors.get(&key) != Some(&e) {
                tracing::warn!("orquestrador: {key}: {e}");
            }
            errors.insert(key, e);
            Vec::new()
        }
    }
}

/// Abre trabalhadores para as próximas tarefas `agente` enquanto houver vaga em "fazendo".
/// Devolve as tarefas que começaram. Tarefa que já voltou do "fazendo" fica (`next_auto`).
async fn orchestrate(
    state: &AppState,
    profile: &str,
    project: &str,
) -> Result<Vec<String>, String> {
    let _turn = state.0.orchestrator_lock.lock().await;
    let dir = match project_dir(state, profile, project) {
        Ok(dir) => dir,
        Err(response) => return Err(reply_text(response).await),
    };
    let mut started = Vec::new();
    // Cada volta abre no máximo o limite; trabalhador que morre na hora volta com `failures`
    // e não é repuxado, então o laço sempre termina.
    for _ in 0..hub_core::kanban::MAX_WORKERS {
        let next = {
            let _guard = state.0.kanban_lock.lock().await;
            let board = match load(state, &dir).await {
                Ok(board) => board,
                Err(response) => return Err(reply_text(response).await),
            };
            let limit = board.orchestrator.max_workers as usize;
            if !board.orchestrator.enabled || board.in_progress() >= limit {
                break;
            }
            match board.next_auto() {
                Some(card) => card.id.clone(),
                None => break,
            }
        };
        if let Err(response) = start_worker(state, profile, project, &next, Vec::new(), None).await
        {
            return Err(format!("{next}: {}", reply_text(response).await));
        }
        started.push(next);
    }
    Ok(started)
}

/// A mensagem de um erro da API (`{"error": …}`), para o log e o quadro.
async fn reply_text(response: Response) -> String {
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .map(|body| body.to_bytes())
        .unwrap_or_default();
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("erro {status}"))
}

#[cfg(test)]
mod tests {
    use super::super::{AppState, agent_router, router};
    use super::*;
    use crate::sessions::tests::TestTmux;
    use crate::sleep::tests::{git_sync, init_repo};
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use hub_core::worker;
    use hub_core::worker::WorkerRun;
    use hub_core::{AgentKind, Config, SessionSpec, exec_client_dir};
    use tower::ServiceExt;

    const EXAMPLE_TOML: &str = include_str!("../../../../config/assistente.exemplo.toml");

    struct Hub {
        /// API do app (UI), sem token.
        app: Router,
        /// API dos agentes (`agent_listen`).
        agents: Router,
        /// Repo `proj` de verdade (git), perfil `pessoal`.
        proj: PathBuf,
        home: PathBuf,
    }

    /// `exec` liga a API dos agentes, sem a qual o trabalhador não abre (como na VM).
    fn hub(t: &TestTmux, exec: bool) -> Hub {
        hub_com(t, exec, vec!["bash".into(), "--norc".into(), "-i".into()])
    }

    /// O mesmo hub, mas com outro comando no lugar do agente (o trabalhador também usa).
    fn hub_com(t: &TestTmux, exec: bool, argv: Vec<String>) -> Hub {
        let root = t.dir.join("raiz");
        let proj = root.join("proj");
        init_repo(&proj);
        let home = t.dir.join("home");
        let text =
            EXAMPLE_TOML.replace("~/Documentos/pessoal/projetos", &root.display().to_string());
        let config = Config::parse(&text, &home).unwrap();
        let mut state = AppState::new(config, home.clone(), None, None, t.tmux.clone())
            .with_argv_override(argv);
        if exec {
            let client = exec_client_dir(&home);
            std::fs::create_dir_all(&client).unwrap();
            std::fs::write(client.join("fora-do-jail"), "").unwrap();
            state = state.with_exec_port(Some(7879));
        }
        Hub {
            app: router(state.clone()),
            agents: agent_router(state),
            proj,
            home,
        }
    }

    async fn send(
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

    /// Sessão de agente de verdade no tmux do teste, com o token da API dos agentes.
    async fn session(t: &TestTmux, agent: AgentKind, seq: u32) -> (String, String) {
        let spec = SessionSpec {
            profile: "pessoal".into(),
            project: "proj".into(),
            agent,
        };
        let id = format!("pessoal--proj--{}-{seq}", agent.id());
        let token = crate::sessions::new_exec_token().unwrap();
        t.tmux
            .create(&t.plan(&id, "sleep 300"), &spec, Some(&token))
            .await
            .unwrap();
        (id, token)
    }

    async fn exec_token(t: &TestTmux, session: &str) -> String {
        let out = t
            .tmux
            .command(&[
                "show-options".into(),
                "-v".into(),
                "-t".into(),
                format!("={session}:"),
                "@assistente_exec_token".into(),
            ])
            .output()
            .await
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    fn last_subject(proj: &Path) -> String {
        git_sync(proj, &["log", "-1", "--format=%s"])
    }

    const CARDS: &str = "/api/projects/pessoal/proj/kanban";

    #[tokio::test]
    async fn kanban_crud_commits_each_change_alone() {
        let t = TestTmux::new("kanban-crud");
        let hub = hub(&t, false);
        // Algo já em andamento no index não pode entrar no commit do quadro.
        std::fs::write(hub.proj.join("solto.txt"), "x").unwrap();
        git_sync(&hub.proj, &["add", "solto.txt"]);

        let (status, body) = send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"Primeira","kind":"agente"}"#),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["id"], "T1");
        assert_eq!(body["column"], "backlog");
        assert_eq!(last_subject(&hub.proj), "kanban: cria T1");
        // A lista de projetos conta o que falta no quadro (F11.3).
        let (_, listed) = send(
            hub.app.clone(),
            "GET",
            "/api/projects?profile=pessoal",
            None,
            None,
        )
        .await;
        let proj = listed
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "proj")
            .unwrap();
        assert_eq!(proj["kanban_pendentes"], 1, "{listed}");
        assert_eq!(
            git_sync(&hub.proj, &["show", "--name-only", "--format="]),
            ".assistente/kanban.json"
        );
        assert_eq!(
            git_sync(&hub.proj, &["diff", "--cached", "--name-only"]),
            "solto.txt"
        );

        let card = format!("{CARDS}/T1");
        let (status, body) = send(
            hub.app.clone(),
            "PATCH",
            &card,
            Some(r#"{"column":"fazendo"}"#),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(last_subject(&hub.proj), "kanban: move T1 para fazendo");

        let (status, _) = send(
            hub.app.clone(),
            "PATCH",
            &card,
            Some(r#"{"title":"Nova"}"#),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(last_subject(&hub.proj), "kanban: edita T1");

        let (status, _) = send(hub.app.clone(), "DELETE", &card, None, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(last_subject(&hub.proj), "kanban: apaga T1");
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(board["cards"], json!([]));

        // Título vazio é recusado e não gera commit.
        let commits = git_sync(&hub.proj, &["rev-list", "--count", "HEAD"]);
        let (status, _) = send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"  "}"#),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            git_sync(&hub.proj, &["rev-list", "--count", "HEAD"]),
            commits
        );
    }

    #[tokio::test]
    async fn pull_gives_agent_cards_and_a_dead_session_gives_its_card_back() {
        let t = TestTmux::new("kanban-pull");
        let hub = hub(&t, false);
        send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"Manual"}"#),
            None,
        )
        .await;
        send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"Para o agente","kind":"agente"}"#),
            None,
        )
        .await;
        let (s1, tok1) = session(&t, AgentKind::Claude, 1).await;

        // Manual nunca é puxado: só o card marcado para o agente.
        let (status, body) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/pull",
            None,
            Some(&tok1),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["card"]["id"], "T2");
        assert_eq!(body["card"]["holder"], s1.as_str());

        // Sessão morre: a tarefa volta ao backlog na próxima leitura, com commit.
        t.tmux.kill(&s1).await.unwrap();
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        let card = board["cards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "T2")
            .unwrap();
        assert_eq!(card["column"], "backlog");
        assert!(card.get("holder").is_none());
        assert_eq!(
            last_subject(&hub.proj),
            "kanban: solta T2 (sessão encerrada)"
        );
    }

    #[tokio::test]
    async fn only_the_holder_finishes_or_releases_and_empty_board_is_null() {
        let t = TestTmux::new("kanban-posse");
        let hub = hub(&t, false);
        for title in ["um", "dois"] {
            let body = format!(r#"{{"title":"{title}","kind":"agente"}}"#);
            send(hub.app.clone(), "POST", CARDS, Some(&body), None).await;
        }
        let (_, tok1) = session(&t, AgentKind::Claude, 1).await;
        let (_, tok2) = session(&t, AgentKind::Opencode, 1).await;
        let pull = |token: String| {
            let app = hub.agents.clone();
            async move { send(app, "POST", "/v1/kanban/pull", None, Some(&token)).await }
        };
        assert_eq!(pull(tok1.clone()).await.1["card"]["id"], "T1");
        assert_eq!(pull(tok2.clone()).await.1["card"]["id"], "T2");
        assert_eq!(pull(tok1.clone()).await.1["card"], serde_json::Value::Null);

        let done = |token: String, body: &'static str| {
            let app = hub.agents.clone();
            async move { send(app, "POST", "/v1/kanban/done", Some(body), Some(&token)).await }
        };
        assert_eq!(
            done(tok2.clone(), r#"{"id":"T1"}"#).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            done(tok2.clone(), r#"{"id":"T2","commit":"zz"}"#).await.0,
            StatusCode::BAD_REQUEST
        );
        let (status, body) = done(tok1.clone(), r#"{"id":"T1","commit":"abc1234"}"#).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["column"], "feito");
        assert_eq!(body["commit"], "abc1234");
        assert_eq!(last_subject(&hub.proj), "kanban: T1 concluída");

        let release = |token: String, body: &'static str| {
            let app = hub.agents.clone();
            async move { send(app, "POST", "/v1/kanban/release", Some(body), Some(&token)).await }
        };
        assert_eq!(
            release(tok1.clone(), r#"{"id":"T1"}"#).await.0,
            StatusCode::CONFLICT,
            "feito não se solta"
        );
        assert_eq!(
            release(tok2.clone(), r#"{"id":"T2"}"#).await.0,
            StatusCode::OK
        );
        assert_eq!(last_subject(&hub.proj), "kanban: T2 devolvida ao backlog");
        assert_eq!(pull(tok2).await.1["card"]["id"], "T2");
    }

    #[tokio::test]
    async fn mandar_without_models_picks_the_free_ones_by_history() {
        let t = TestTmux::new("kanban-auto");
        let hub = hub(&t, true);
        // Catálogo do opencode: dois grátis do provedor opencode, um pago e um grátis de outro
        // provedor (que não entra).
        let catalogue = hub.home.join(".cache/opencode/models.json");
        std::fs::create_dir_all(catalogue.parent().unwrap()).unwrap();
        std::fs::write(
            &catalogue,
            r#"{"opencode": {"models": {
                "big-pickle": {"cost": {"input": 0, "output": 0}},
                "novo-free": {"cost": {"input": 0, "output": 0}},
                "pago": {"cost": {"input": 0.4, "output": 2.5}}
            }}, "9router": {"models": {"gpt-5": {"cost": {"input": 0, "output": 0}}}}}"#,
        )
        .unwrap();
        // Histórico do perfil: o grátis big-pickle deu certo; o novo-free falhou.
        let trab = hub.home.join(".assistente/perfis/pessoal/trabalhadores");
        std::fs::create_dir_all(&trab).unwrap();
        std::fs::write(
            trab.join("antiga.modelos"),
            "opencode/novo-free falhou\nopencode/big-pickle ok\n",
        )
        .unwrap();
        send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"Tela de login"}"#),
            None,
        )
        .await;

        let (status, info) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T1/mandar"),
            Some("{}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        let files = worker::files(
            &hub.home.join(".assistente/perfis/pessoal"),
            info["id"].as_str().unwrap(),
        );
        let run: WorkerRun =
            serde_json::from_str(&std::fs::read_to_string(&files.run).unwrap()).unwrap();
        assert_eq!(
            run.models,
            vec![
                "opencode/big-pickle".to_owned(),
                "opencode/novo-free".to_owned()
            ],
            "grátis do opencode só, o melhor pelo histórico primeiro"
        );
    }

    #[tokio::test]
    async fn mandar_opens_a_worker_on_a_worktree_and_restart_keeps_the_card() {
        let t = TestTmux::new("kanban-mandar");
        let hub = hub(&t, true);
        std::fs::write(
            hub.proj.join(".assistente.toml"),
            "[workers]\nmodels = [\"x/a\", \"x/b\"]\nattempts = 2\n",
        )
        .unwrap();
        git_sync(&hub.proj, &["add", ".assistente.toml"]);
        git_sync(&hub.proj, &["commit", "-q", "-m", "config do projeto"]);
        send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"Tela de login","description":"e-mail e senha"}"#),
            None,
        )
        .await;

        // Mandar: manual vai para um trabalhador com os modelos do projeto.
        let (status, info) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T1/mandar"),
            Some("{}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        let worker = info["id"].as_str().unwrap().to_owned();
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(board["cards"][0]["column"], "fazendo");
        assert_eq!(board["cards"][0]["holder"], worker.as_str());

        let worktree = hub.proj.join(".assistente/worktrees/T1");
        assert!(worktree.join(".git").exists());
        assert!(git_sync(&hub.proj, &["branch", "--list", "agente/T1"]).contains("agente/T1"));
        assert_eq!(
            git_sync(&hub.proj, &["status", "--porcelain"]),
            "",
            "worktree fica fora do git do projeto"
        );
        let files = worker::files(&hub.home.join(".assistente/perfis/pessoal"), &worker);
        let prompt = std::fs::read_to_string(&files.prompt).unwrap();
        assert!(prompt.starts_with("# Tarefa T1: Tela de login"), "{prompt}");
        let run: WorkerRun =
            serde_json::from_str(&std::fs::read_to_string(&files.run).unwrap()).unwrap();
        assert_eq!(run.models, vec!["x/a".to_owned(), "x/b".to_owned()]);
        assert_eq!(run.attempts, 2);
        assert_eq!(run.worktree, worktree);

        // Já está em andamento: nova tentativa de Mandar recusa.
        let (status, _) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T1/mandar"),
            Some("{}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);

        // Reiniciar: outra sessão assume a tarefa, a antiga sai do tmux.
        let (status, restarted) = send(
            hub.app.clone(),
            "POST",
            &format!("/api/sessions/{worker}/reiniciar"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{restarted}");
        let new_worker = restarted["id"].as_str().unwrap().to_owned();
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(board["cards"][0]["holder"], new_worker.as_str());

        // Sessão nova morre: a tarefa volta ao backlog.
        t.tmux.kill(&new_worker).await.unwrap();
        let (status, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(status, StatusCode::OK, "{board}");
        assert_eq!(board["cards"][0]["column"], "backlog");
    }

    #[tokio::test]
    async fn orchestrator_hands_the_card_to_a_worker_and_workers_stop_there() {
        let t = TestTmux::new("kanban-orquestra");
        let hub = hub(&t, true);
        std::fs::write(
            hub.proj.join(".assistente.toml"),
            "[workers]\nmodels = [\"x/a\"]\n",
        )
        .unwrap();
        git_sync(&hub.proj, &["add", ".assistente.toml"]);
        git_sync(&hub.proj, &["commit", "-q", "-m", "config do projeto"]);
        send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"Cadastro","kind":"agente"}"#),
            None,
        )
        .await;
        let (_, claude) = session(&t, AgentKind::Claude, 1).await;
        let (status, pulled) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/pull",
            None,
            Some(&claude),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{pulled}");

        // O orquestrador abre o trabalhador: a tarefa muda de dono.
        let body = r#"{"task":"T1","models":["x/a"]}"#;
        let (status, info) = send(
            hub.agents.clone(),
            "POST",
            "/v1/workers",
            Some(body),
            Some(&claude),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        let worker = info["id"].as_str().unwrap().to_owned();
        let worker_token = exec_token(&t, &worker).await;
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(board["cards"][0]["holder"], worker.as_str());

        // Trabalhador não abre outro, e conclui a própria tarefa.
        let (status, _) = send(
            hub.agents.clone(),
            "POST",
            "/v1/workers",
            Some(body),
            Some(&worker_token),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, done) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/done",
            Some(r#"{"id":"T1","commit":"abc1234"}"#),
            Some(&worker_token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert_eq!(done["column"], "feito");
    }

    #[tokio::test]
    async fn orchestrator_fills_up_to_the_limit_and_leaves_failed_cards_alone() {
        let t = TestTmux::new("kanban-auto-orquestra");
        let hub = hub(&t, true);
        std::fs::write(
            hub.proj.join(".assistente.toml"),
            "[workers]\nmodels = [\"x/a\"]\n",
        )
        .unwrap();
        git_sync(&hub.proj, &["add", ".assistente.toml"]);
        git_sync(&hub.proj, &["commit", "-q", "-m", "config do projeto"]);
        for body in [
            r#"{"title":"um","kind":"agente"}"#,
            r#"{"title":"dois","kind":"agente","priority":"alta"}"#,
            r#"{"title":"tres","kind":"agente","priority":"baixa"}"#,
            r#"{"title":"manual"}"#,
        ] {
            send(hub.app.clone(), "POST", CARDS, Some(body), None).await;
        }
        let column = |board: &serde_json::Value, id: &str| -> String {
            let card = board["cards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap();
            card["column"].as_str().unwrap().to_owned()
        };

        let uri = format!("{CARDS}/orquestrador");
        let (status, board) = send(
            hub.app.clone(),
            "PUT",
            &uri,
            Some(r#"{"enabled":true,"max_workers":2,"instructions":" Use pnpm. "}"#),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{board}");
        assert_eq!(board["orchestrator"]["enabled"], true);
        assert_eq!(board["orchestrator"]["max_workers"], 2);
        assert_eq!(board["orchestrator"]["instructions"], "Use pnpm.");
        // A resposta diz o que a volta abriu, para o aviso da UI.
        assert_eq!(board["started"], json!(["T2", "T1"]), "{board}");
        // Ligar já dá a primeira volta: as duas primeiras pela ordem do pull, a manual nunca.
        assert_eq!(column(&board, "T2"), "fazendo", "{board}");
        assert_eq!(column(&board, "T1"), "fazendo", "{board}");
        assert_eq!(column(&board, "T3"), "backlog");
        assert_eq!(column(&board, "T4"), "backlog");
        assert!(board.get("orchestrator_error").is_none(), "{board}");
        assert!(
            git_sync(&hub.proj, &["log", "--format=%s"])
                .contains("kanban: orquestrador ligado (até 2 em fazendo)")
        );

        // O trabalhador do T2 desiste: T2 volta ao backlog e o T3 entra na vaga, o T2 não.
        let worker = board["cards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "T2")
            .unwrap()["holder"]
            .as_str()
            .unwrap()
            .to_owned();
        // As instruções do projeto vão no fim das regras padrão do trabalhador.
        let files = worker::files(&hub.home.join(".assistente/perfis/pessoal"), &worker);
        let prompt = std::fs::read_to_string(&files.prompt).unwrap();
        assert!(
            prompt.ends_with("## Instruções do projeto\n\nUse pnpm.\n"),
            "{prompt}"
        );
        let token = exec_token(&t, &worker).await;
        let (status, _) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/release",
            Some(r#"{"id":"T2"}"#),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        t.tmux.kill(&worker).await.unwrap();
        let (_, board) = send(
            hub.app.clone(),
            "PUT",
            &uri,
            Some(r#"{"enabled":true,"max_workers":2}"#),
            None,
        )
        .await;
        assert_eq!(column(&board, "T3"), "fazendo", "{board}");
        assert_eq!(column(&board, "T2"), "backlog");
        assert_eq!(board["cards"][1]["failures"], 1, "{board}");

        // "Tentar de novo" zera a contagem; com vaga, a próxima volta pega o T2.
        send(
            hub.app.clone(),
            "PATCH",
            &format!("{CARDS}/T2"),
            Some(r#"{"failures":0}"#),
            None,
        )
        .await;
        let (_, board) = send(
            hub.app.clone(),
            "PUT",
            &uri,
            Some(r#"{"enabled":true,"max_workers":3}"#),
            None,
        )
        .await;
        assert_eq!(column(&board, "T2"), "fazendo", "{board}");
        assert_eq!(board["started"], json!(["T2"]));
        assert_eq!(
            board["orchestrator"]["instructions"], "Use pnpm.",
            "mantidas"
        );

        // Desligado, nada novo abre e o quadro fica como está.
        let (_, board) = send(
            hub.app.clone(),
            "PUT",
            &uri,
            Some(r#"{"enabled":false,"instructions":""}"#),
            None,
        )
        .await;
        assert!(board.get("orchestrator").is_none(), "{board}");
        assert!(board.get("started").is_none(), "{board}");
        assert_eq!(last_subject(&hub.proj), "kanban: orquestrador desligado");
    }

    #[tokio::test]
    async fn orchestrator_error_shows_on_the_board() {
        let t = TestTmux::new("kanban-auto-erro");
        // Sem a API dos agentes o trabalhador não abre: o erro vai para o quadro.
        let hub = hub(&t, false);
        std::fs::write(
            hub.proj.join(".assistente.toml"),
            "[workers]\nmodels = [\"x/a\"]\n",
        )
        .unwrap();
        git_sync(&hub.proj, &["add", ".assistente.toml"]);
        git_sync(&hub.proj, &["commit", "-q", "-m", "config do projeto"]);
        send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"um","kind":"agente"}"#),
            None,
        )
        .await;
        let (status, board) = send(
            hub.app.clone(),
            "PUT",
            &format!("{CARDS}/orquestrador"),
            Some(r#"{"enabled":true}"#),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{board}");
        let message = board["orchestrator_error"].as_str().unwrap_or_default();
        assert!(message.starts_with("T1: a API dos agentes"), "{board}");
        assert_eq!(board["cards"][0]["column"], "backlog");
        let (_, listed) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(listed["orchestrator_error"], board["orchestrator_error"]);
    }

    #[tokio::test]
    async fn explorar_opens_a_claude_that_starts_with_the_backlog_prompt() {
        let t = TestTmux::new("kanban-explorar");
        let hub = hub(&t, true);
        let uri = "/api/projects/pessoal/proj/kanban/explorar";
        let (status, _) = send(
            hub.app.clone(),
            "POST",
            "/api/projects/pessoal/nada/kanban/explorar",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, info) = send(hub.app.clone(), "POST", uri, None, None).await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        assert_eq!(info["agent"], "claude");
        // O prompt vai no argv do claude (testado em hub-core); aqui só confere a sessão viva.
        assert!(
            t.tmux
                .find(info["id"].as_str().unwrap())
                .await
                .unwrap()
                .is_some()
        );
        // E fica gravado na pasta do perfil, que é o que o diagnóstico lista (F14.1).
        let arquivo = hub
            .home
            .join(".assistente/perfis/pessoal/explorar")
            .join(format!("{}.md", info["id"].as_str().unwrap()));
        let prompt = std::fs::read_to_string(&arquivo).unwrap();
        assert!(prompt.contains("montar o quadro de tarefas"), "{prompt}");
    }

    #[tokio::test]
    async fn agents_list_and_create_cards_with_their_session_token() {
        let t = TestTmux::new("kanban-criar");
        let hub = hub(&t, false);
        let (_, claude) = session(&t, AgentKind::Claude, 1).await;

        let create = |body: &'static str, token: Option<String>| {
            let app = hub.agents.clone();
            async move {
                send(
                    app,
                    "POST",
                    "/v1/kanban/criar",
                    Some(body),
                    token.as_deref(),
                )
                .await
            }
        };
        assert_eq!(
            create(r#"{"title":"Login"}"#, None).await.0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            create(r#"{"title":"  "}"#, Some(claude.clone())).await.0,
            StatusCode::BAD_REQUEST
        );
        let (status, card) = create(
            r#"{"title":"Login","kind":"agente","priority":"alta"}"#,
            Some(claude.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{card}");
        assert_eq!(card["id"], "T1");
        assert_eq!(card["kind"], "agente");
        assert_eq!(
            last_subject(&hub.proj),
            "kanban: cria T1 (por pessoal--proj--claude-1)"
        );

        let (status, board) =
            send(hub.agents.clone(), "GET", "/v1/kanban", None, Some(&claude)).await;
        assert_eq!(status, StatusCode::OK, "{board}");
        assert_eq!(board["cards"][0]["title"], "Login");
    }

    /// Roda o `vm/fora-do-jail` de verdade contra a API dos agentes: pega o quoting e o JSON.
    #[tokio::test]
    async fn fora_do_jail_kanban_talks_to_the_board() {
        let t = TestTmux::new("kanban-cliente");
        let hub = hub(&t, false);
        let (_, claude) = session(&t, AgentKind::Claude, 1).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = hub.agents.clone();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = |args: &[&str]| {
            let url = url.clone();
            let token = claude.clone();
            let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
            async move {
                tokio::process::Command::new("bash")
                    .arg(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../vm/fora-do-jail"
                    ))
                    .arg("kanban")
                    .args(&args)
                    .env("ASSISTENTE_EXEC_URL", url)
                    .env("ASSISTENTE_EXEC_TOKEN", token)
                    .output()
                    .await
                    .unwrap()
            }
        };
        let out = client(&[
            "criar",
            "--titulo",
            "Tela de \"login\"",
            "--descricao",
            "e-mail, senha; \"pronto\" quando passar",
            "--tipo",
            "agente",
        ])
        .await;
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let card: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(card["title"], "Tela de \"login\"");
        assert_eq!(
            card["description"],
            "e-mail, senha; \"pronto\" quando passar"
        );
        assert_eq!(card["kind"], "agente");

        let out = client(&["listar"]).await;
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let board: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(board["cards"].as_array().unwrap().len(), 1);

        let out = client(&["criar"]).await;
        assert!(!out.status.success(), "criar sem título precisa falhar");
    }

    /// F14.4: o trabalhador que termina bem fecha a própria sessão (não sobra aba morta na
    /// UI) e o Claude que sai com erro continua no tmux, marcado como morto.
    #[tokio::test]
    async fn worker_that_finishes_leaves_no_dead_session_and_a_failing_claude_stays() {
        let t = TestTmux::new("kanban-viva");
        // O trabalhador tem `AGENTE_TAREFA` no env: sai com 0. O Claude não tem: sai com 127.
        let hub = hub_com(
            &t,
            true,
            vec![
                "bash".into(),
                "-c".into(),
                "if [ -n \"${AGENTE_TAREFA:-}\" ]; then sleep 0.5; exit 0; fi; \
                 echo 'claude: not found'; exit 127"
                    .into(),
            ],
        );
        let (status, _) = send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"Tarefa"}"#),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        // O trabalhador precisa de modelo: o do projeto (sem o catálogo do opencode).
        std::fs::write(
            hub.proj.join(".assistente.toml"),
            "[workers]\nmodels = [\"x/a\"]\n",
        )
        .unwrap();
        git_sync(&hub.proj, &["add", ".assistente.toml"]);
        git_sync(&hub.proj, &["commit", "-q", "-m", "config do projeto"]);

        // "Mandar": o trabalhador abre e sai com 0.
        let (status, info) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T1/mandar"),
            Some("{}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        let worker = info["id"].as_str().unwrap().to_owned();
        assert_eq!(worker, "pessoal--proj--opencode-1");

        // A sessão do trabalhador morre com ele: a lista fica sem aba morta.
        let mut sessions = serde_json::Value::Null;
        for _ in 0..50 {
            let (_, listed) = send(hub.app.clone(), "GET", "/api/sessions", None, None).await;
            sessions = listed;
            if sessions.as_array().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(
            sessions.as_array().unwrap().is_empty(),
            "o trabalhador que terminou bem deixou sessão morta: {sessions}"
        );

        // O Claude que sai com erro fica: a sessão continua, morta, com o painel na tela.
        let spec = r#"{"profile":"pessoal","project":"proj","agent":"claude"}"#;
        let (status, body) = send(hub.app.clone(), "POST", "/api/sessions", Some(spec), None).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let (_, sessions) = send(hub.app.clone(), "GET", "/api/sessions", None, None).await;
        let claude = &sessions.as_array().unwrap()[0];
        assert_eq!(claude["id"], "pessoal--proj--claude-1");
        assert_eq!(claude["dead"], true, "{claude}");
        // A saída do processo continua no painel.
        let pane = t
            .tmux
            .capture_pane("pessoal--proj--claude-1", -20)
            .await
            .unwrap();
        assert!(pane.contains("claude: not found"), "{pane}");
    }

    /// F14.4: a sessão que desiste da tarefa fica com o painel na tela, para o erro ficar
    /// visível depois que o processo morre.
    #[tokio::test]
    async fn releasing_a_task_keeps_the_panel_of_the_session() {
        let t = TestTmux::new("kanban-painel");
        let hub = hub(&t, true);
        let (id, token) = session(&t, AgentKind::Opencode, 1).await;
        // A sessão segura a tarefa e desiste dela.
        send(
            hub.app.clone(),
            "POST",
            CARDS,
            Some(r#"{"title":"um","kind":"agente"}"#),
            None,
        )
        .await;
        send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/pull",
            None,
            Some(&token),
        )
        .await;
        let (status, _) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/release",
            Some(r#"{"id":"T1"}"#),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        // A janela da sessão ficou com o `remain-on-exit`: o painel não some com o processo.
        let option = t
            .tmux
            .command(&[
                "show-options".into(),
                "-w".into(),
                "-v".into(),
                "-t".into(),
                format!("={id}:"),
                "remain-on-exit".into(),
            ])
            .output()
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&option.stdout).trim(),
            "on",
            "a sessão que desiste fica com o painel"
        );
    }
}
