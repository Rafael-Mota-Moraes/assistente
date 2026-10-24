//! Quadro dos projetos e trabalhadores (F11). Da UI: `/api/projects/{perfil}/{projeto}/kanban…`,
//! `…/kanban/{id}/mandar` e `/api/sessions/{id}/reiniciar`. Da API dos agentes (token da sessão):
//! `/v1/kanban/{pull,done,release,block}` e `/v1/workers`, que só o orquestrador (sessão do
//! Claude) usa. Cota esgotada (F11.14): `block` pausa o orquestrador; `…/orquestrador/retomar`
//! tira a pausa e `…/kanban/{id}/claude` termina a tarefa com um Claude no mesmo worktree.
//! QA (F11.15): `…/kanban/{id}/qa` abre um Claude que confere um card "feito"; ele chama
//! `/v1/kanban/validate` (o hubd junta a branch e o card vai para "validado") ou
//! `/v1/kanban/reopen` (volta ao backlog com o que falta). `…/kanban/qa` (F11.16) é o QA geral:
//! um Claude só, na raiz do projeto, segurando todos os cards "feitos" livres.
//! O orquestrador automático (F11.11, `spawn_orchestrator`) é um laço do próprio hubd: liga em
//! `PUT …/kanban/orquestrador` e abre trabalhadores para as tarefas `agente`, sem sessão do Claude.
//! Regras em `hub_core::kanban`; arquivo e commit em `crate::kanban`; trabalhador em `hub_core::worker`.

// Os handlers devolvem `Response` como o resto de `api.rs`; o `Err` grande é o preço disso.
#![allow(clippy::result_large_err)]

use super::*;
use crate::kanban as store;
use crate::sessions::PaneState;
use axum::extract::Path as Params;
use hub_core::kanban::{Board, Card, Column, Draft, KanbanError, Merged};
use hub_core::worker::{self, WorkerRun};
use hub_core::{AgentKind, profile_data_dir, workers_config_at};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        Column::Validando => "validando",
        Column::Validado => "validado",
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
                clean_validated(dir, &board, &released);
            }
        }
        Err(e) => tracing::warn!("quadro: sem a lista de sessões, nada foi solto: {e}"),
    }
    Ok(board)
}

/// Worktree e branch de uma tarefa validada saem quando a sessão do QA acaba (F11.15). O git
/// recusa remover worktree com mudança, e a branch só sai se já está no commit juntado.
fn clean_validated(dir: &Path, board: &Board, released: &[String]) {
    for card in board.validated_worktrees() {
        if !released.contains(&card.id) {
            continue;
        }
        let worktree = worker::worktree_dir(dir, &card.id);
        if worktree.exists() {
            let path = worktree.display().to_string();
            if let Err(e) = git_out(dir, &["worktree", "remove", &path]) {
                tracing::warn!("worktree de {} ficou: {e}", card.id);
                continue;
            }
        }
        let branch = worker::branch(&card.id);
        let merged = card.merged.as_ref().map(|m| m.commit.as_str());
        if let Some(merged) = merged
            && git_out(dir, &["merge-base", "--is-ancestor", &branch, merged]).is_ok()
        {
            // Tenta deletar com -d (seguro, só se já mesclado); cai para -D se falhar.
            if git_out(dir, &["branch", "-d", &branch]).is_err()
                && let Err(e) = git_out(dir, &["branch", "-D", &branch])
            {
                tracing::warn!("branch {branch} ficou: {e}");
            }
        }
    }
}

/// Dependência na `main` (F11.13): o commit que o QA juntou está no `HEAD` do projeto, de onde o
/// worktree do trabalhador nasce. Juntada noutra branch não conta.
fn landed(dir: &Path) -> impl Fn(&Merged) -> bool + '_ {
    move |merged| {
        git_out(
            dir,
            &["merge-base", "--is-ancestor", &merged.commit, "HEAD"],
        )
        .is_ok()
    }
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
    /// Último resumo de revisão (F11.12), lido de `.assistente/revisoes`.
    #[serde(skip_serializing_if = "Option::is_none")]
    review: Option<ReviewSummary>,
}

fn board_view(state: &AppState, profile: &str, project: &str, board: Board) -> BoardView {
    let review = project_dir(state, profile, project)
        .ok()
        .and_then(|dir| latest_review(&dir));
    let errors = state.0.orchestrator_errors.lock().expect("lock dos erros");
    BoardView {
        orchestrator_error: errors.get(&orchestrator_key(profile, project)).cloned(),
        board,
        started: Vec::new(),
        review,
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

/// POST /api/projects/{perfil}/{projeto}/kanban/orquestrador/retomar: tira a pausa de cota
/// (F11.14) e, ligado, já dá uma volta antes de responder (`started`), como ao ligar.
pub(super) async fn resume_orchestrator(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
) -> Reply<Json<BoardView>> {
    let dir = project_dir(&state, &profile, &project)?;
    {
        let _guard = state.0.kanban_lock.lock().await;
        let mut board = load(&state, &dir).await?;
        board.resume();
        persist(&dir, &board, "kanban: orquestrador retomado")?;
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

/// POST /api/projects/{perfil}/{projeto}/kanban/{id}/claude: "Terminar com… Claude Code"
/// (F11.14). Um Claude abre no worktree da tarefa, continua o que o trabalhador deixou e marca o
/// card ele mesmo (`kanban done`). Vale para qualquer card do backlog, bloqueado ou não.
pub(super) async fn finish_with_claude(
    State(state): State<AppState>,
    Params((profile, project, id)): Params<(String, String, String)>,
) -> Reply<(StatusCode, Json<SessionInfo>)> {
    let dir = project_dir(&state, &profile, &project)?;
    if !worker::valid_task_id(&id) {
        return Err(error(StatusCode::BAD_REQUEST, "id de tarefa inválido"));
    }
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board
        .get(&id)
        .cloned()
        .ok_or_else(|| kanban_error(KanbanError::NotFound))?;
    if card.column != Column::Backlog {
        return Err(kanban_error(KanbanError::NotInBacklog(card.column)));
    }
    let branch = worker::branch(&id);
    let prompt = worker::render_finish_prompt(&card, &branch, &board.orchestrator.instructions);
    let info = claude_in_worktree(&state, &dir, &profile, &project, &id, &prompt).await?;
    let message = format!("kanban: {id} com Claude {}", info.id);
    let saved = board
        .claim(&id, &info.id)
        .map(|_| ())
        .map_err(kanban_error)
        .and_then(|_| persist(&dir, &board, &message));
    if let Err(response) = saved {
        let _ = state.0.tmux.kill(&info.id).await;
        return Err(response);
    }
    tracing::info!("Claude {} terminando a tarefa {id}", info.id);
    Ok((StatusCode::CREATED, Json(info)))
}

/// POST /api/projects/{perfil}/{projeto}/kanban/{id}/qa (F11.15): um Claude no worktree de uma
/// tarefa "feita" pergunta onde juntar, confere o trabalho contra o pedido e valida ou reabre.
/// Segura o card enquanto confere: um QA por vez.
pub(super) async fn qa_card(
    State(state): State<AppState>,
    Params((profile, project, id)): Params<(String, String, String)>,
) -> Reply<(StatusCode, Json<SessionInfo>)> {
    let dir = project_dir(&state, &profile, &project)?;
    if !worker::valid_task_id(&id) {
        return Err(error(StatusCode::BAD_REQUEST, "id de tarefa inválido"));
    }
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board
        .get(&id)
        .cloned()
        .ok_or_else(|| kanban_error(KanbanError::NotFound))?;
    if card.column != Column::Feito {
        return Err(kanban_error(KanbanError::NotDone(card.column)));
    }
    if card.holder.is_some() {
        return Err(kanban_error(KanbanError::NotHolder));
    }
    let branch = worker::branch(&id);
    let prompt = worker::render_qa_prompt(&card, &branch, &board.orchestrator.instructions);
    let info = claude_in_worktree(&state, &dir, &profile, &project, &id, &prompt).await?;
    let message = format!("kanban: QA de {id} com {}", info.id);
    let saved = board
        .start_qa(&id, &info.id)
        .map(|_| ())
        .map_err(kanban_error)
        .and_then(|_| persist(&dir, &board, &message));
    if let Err(response) = saved {
        let _ = state.0.tmux.kill(&info.id).await;
        return Err(response);
    }
    tracing::info!("Claude {} no QA da tarefa {id}", info.id);
    Ok((StatusCode::CREATED, Json(info)))
}

/// POST /api/projects/{perfil}/{projeto}/kanban/qa (F11.16): QA geral, para limpar o quadro. Um
/// Claude na raiz do projeto segura todos os cards "feitos" sem dono, pergunta uma vez onde juntar
/// e valida ou reabre cada um. O que ele não tocar volta a ficar livre quando a sessão acabar.
pub(super) async fn qa_all(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
) -> Reply<(StatusCode, Json<SessionInfo>)> {
    let info = hold_done_cards(&state, &profile, &project, false).await?;
    Ok((StatusCode::CREATED, Json(info)))
}

/// POST /api/projects/{perfil}/{projeto}/kanban/revisar (F11.12): a revisão em lote. Um Claude na
/// raiz do projeto, com `--docker` para o check, segura os cards "feitos" livres, junta os que
/// passam na `main` (`/v1/kanban/juntar`), devolve os outros ao backlog e grava o resumo
/// (`/v1/kanban/resumo`). O orquestrador chama o mesmo sozinho (`auto_review`).
pub(super) async fn review_board(
    State(state): State<AppState>,
    Params((profile, project)): Params<(String, String)>,
) -> Reply<(StatusCode, Json<SessionInfo>)> {
    let info = hold_done_cards(&state, &profile, &project, true).await?;
    Ok((StatusCode::CREATED, Json(info)))
}

/// Abre o Claude do QA geral (`review` falso) ou da revisão (verdadeiro) segurando todos os
/// cards "feitos" sem dono, em ordem de número.
async fn hold_done_cards(
    state: &AppState,
    profile: &str,
    project: &str,
    review: bool,
) -> Reply<SessionInfo> {
    let dir = project_dir(state, profile, project)?;
    // Sem a API dos agentes o Claude não conseguiria validar nem reabrir: não abre.
    if exec_access_for(&state.0, AgentKind::Claude).is_none() {
        return Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "a API dos agentes está desligada: o Claude não conseguiria mexer nos cards",
        ));
    }
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(state, &dir).await?;
    let mut cards: Vec<Card> = board
        .cards
        .iter()
        .filter(|card| card.column == Column::Feito && card.holder.is_none())
        .cloned()
        .collect();
    if cards.is_empty() {
        let what = if review { "a revisão" } else { "o QA" };
        return Err(error(
            StatusCode::CONFLICT,
            &format!("nenhum card em feito livre para {what}"),
        ));
    }
    cards.sort_by_key(|card| number(&card.id));
    // A revisão cria a worktree dela em `.assistente/worktrees/`: fora do commit do quadro.
    if review {
        crate::sleep::exclude_line(&dir, worker::WORKTREES_EXCLUDE)
            .await
            .map_err(|e| internal(&format!("{e:#}")))?;
    }
    let instructions = &board.orchestrator.instructions;
    let prompt = if review {
        worker::render_review_prompt(&cards, instructions)
    } else {
        worker::render_qa_all_prompt(&cards, instructions)
    };
    let spec = SessionSpec {
        profile: profile.to_owned(),
        project: project.to_owned(),
        agent: AgentKind::Claude,
    };
    let info = super::launch_session_with(state, &spec, Some(&prompt), review).await?;
    let ids: Vec<&str> = cards.iter().map(|card| card.id.as_str()).collect();
    let what = if review { "revisão" } else { "QA geral" };
    let message = format!("kanban: {what} de {} com {}", ids.join(", "), info.id);
    // Segura os cards (QA ou revisão): o Claude precisa ser o holder para chamar
    // validate/reopen (QA) ou juntar/reabrir/resumo (revisão).
    let saved = ids
        .iter()
        .try_for_each(|id| board.start_qa(id, &info.id).map(|_| ()))
        .map_err(kanban_error)
        .and_then(|_| persist(&dir, &board, &message));
    if let Err(response) = saved {
        let _ = state.0.tmux.kill(&info.id).await;
        return Err(response);
    }
    tracing::info!("Claude {} na {what} de {}", info.id, ids.join(", "));
    Ok(info)
}

fn number(id: &str) -> u32 {
    id.trim_start_matches('T').parse().unwrap_or(u32::MAX)
}

/// Abre um Claude interativo no worktree da tarefa (criado se faltar), com a API dos agentes,
/// sem a qual ele não conseguiria mexer no card. Chamar com `kanban_lock` segurado.
async fn claude_in_worktree(
    state: &AppState,
    dir: &Path,
    profile: &str,
    project: &str,
    id: &str,
    prompt: &str,
) -> Reply<SessionInfo> {
    let inner = &state.0;
    let spec = SessionSpec {
        profile: profile.to_owned(),
        project: project.to_owned(),
        agent: AgentKind::Claude,
    };
    let exec = exec_access_for(inner, AgentKind::Claude).ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "a API dos agentes está desligada: o Claude não conseguiria mexer no card",
        )
    })?;
    let branch = worker::branch(id);
    let worktree = worker::worktree_dir(dir, id);
    ensure_worktree(dir, &worktree, &branch).await?;
    if let Err(e) = sleep::exclude_jail_config(&worktree).await {
        tracing::warn!("sem exclude do .ai-jail em {}: {e:#}", worktree.display());
    }
    let existing = inner.tmux.list().await.map_err(|e| internal(&e))?;
    let token = exec.token.clone();
    let options = LaunchOptions {
        settings: project_settings_for(inner, profile, project),
        exec: Some(exec),
    };
    let mut plan = LaunchPlan::build_finish(
        &inner.config,
        &inner.home,
        &spec,
        next_seq(&existing, &spec),
        &options,
        prompt,
        &worktree,
    )
    .map_err(|e| error(StatusCode::BAD_REQUEST, &e.to_string()))?;
    if let Some(argv) = &inner.argv_override {
        plan.argv = argv.clone();
    }
    inner
        .tmux
        .create(&plan, &spec, Some(&token))
        .await
        .map_err(|e| internal(&e))
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
    // O worktree nasce do `HEAD`: sem as dependências na `main`, sairia sem o código delas.
    if !board.dependencies_satisfied(&card, landed(&dir)) {
        let deps = card.depends_on.join(", ");
        return Err(error(
            StatusCode::CONFLICT,
            &format!("{id} espera {deps} entrar na main (validado pelo QA)"),
        ));
    }
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
    let card = board.pull(&session.id, landed(&dir)).cloned();
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
pub(super) struct BlockBody {
    id: String,
    #[serde(default)]
    reason: String,
}

/// POST /v1/kanban/block `{id, reason}` (F11.14): o provedor recusou por cota. A tarefa que esta
/// sessão segura volta ao backlog sem contar falha, e o orquestrador do projeto pausa.
pub(super) async fn agent_block(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<BlockBody>,
) -> Reply<Json<Card>> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board
        .block(&body.id, &session.id, &body.reason, now())
        .map_err(kanban_error)?
        .clone();
    persist(
        &dir,
        &board,
        &format!("kanban: {} parada por cota; orquestrador pausado", card.id),
    )?;
    tracing::warn!(
        "quadro {}/{}: {} parada por cota; orquestrador pausado",
        session.profile,
        session.project,
        card.id
    );
    Ok(Json(card))
}

#[derive(Deserialize)]
pub(super) struct ValidateBody {
    id: String,
    branch: String,
}

/// Tempo do check do projeto no PC (F11.17). O `fora-do-jail kanban validar` espera mais que
/// isto somado ao prazo de aprovação do pedido.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Fim da saída do check que volta ao QA.
const CHECK_TAIL_LINES: usize = 40;
const CHECK_TAIL_BYTES: usize = 4_000;

/// POST /v1/kanban/validate `{id, branch}` (F11.15, F11.17): o QA que segura a tarefa aprovou.
/// O hubd monta, numa worktree descartável, o commit que a `branch` vai receber (o merge, ou a
/// ponta da tarefa no fast-forward) e roda nela o check do projeto, num pedido `host` que você
/// aprova no app. O card fica "validando" até o resultado. Verde: a `branch` anda (merge local,
/// nunca push) e o card vai para "validado". Vermelho, negado ou expirado: nada muda, o card
/// volta a "feito" e o fim da saída volta ao QA no 409. Conflito = 409, nada muda: o QA junta
/// a `branch` no worktree, resolve e chama de novo.
pub(super) async fn agent_validate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ValidateBody>,
) -> Reply<Json<Card>> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let target = body.branch.trim().to_owned();
    let branch = worker::branch(&body.id);
    if !worker::valid_task_id(&body.id) {
        return Err(error(StatusCode::BAD_REQUEST, "id de tarefa inválido"));
    }
    if target.starts_with('-')
        || target == branch
        || git_out(&dir, &["check-ref-format", "--branch", &target]).is_err()
    {
        return Err(error(StatusCode::BAD_REQUEST, "branch de destino inválida"));
    }
    // A worktree descartável fica com as das tarefas (fora do git): o PC a enxerga no mesmo
    // caminho, e o check roda nela.
    let scratch = worker::worktree_dir(&dir, &format!("qa-juntar-{}-{}", body.id, now()));
    let (card, landing) = {
        let _guard = state.0.kanban_lock.lock().await;
        let mut board = load(&state, &dir).await?;
        // Confere quem segura antes de mexer no git.
        let card = board
            .start_check(&body.id, &session.id)
            .map_err(kanban_error)?
            .clone();
        let worktree = worker::worktree_dir(&dir, &card.id);
        let message = format!("Junta {}: {} (QA)", card.id, card.title);
        // Sem a branch (tarefa feita fora de um trabalhador, p. ex. no modo dormir), vale o commit
        // marcado no card.
        let has_branch = git_out(
            &dir,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )
        .is_ok();
        let source = match (&card.commit, has_branch) {
            (Some(commit), false) => commit.clone(),
            _ => format!("refs/heads/{branch}"),
        };
        // Já está na `target`: nada a juntar nem a verificar.
        if let Some(commit) = already_in(&dir, &source, &target) {
            return validated(&mut board, &dir, &card.id, &session.id, &target, commit);
        }
        // "Validando" antes de montar o merge: o commit do quadro anda com a `target` se ela é o
        // checkout do projeto, e o check vê o que ela vai ser.
        persist(&dir, &board, &format!("kanban: {} validando", card.id))?;
        let prepared = {
            let (dir, target, scratch) = (dir.clone(), target.clone(), scratch.clone());
            tokio::task::spawn_blocking(move || {
                prepare_merge(&dir, &worktree, &source, &target, &scratch, &message)
            })
            .await
            .map_err(|e| internal(&e.to_string()))?
        };
        match prepared {
            Ok(landing) => (card, landing),
            Err((status, message)) => {
                board
                    .check_failed(&card.id, &session.id)
                    .map_err(kanban_error)?;
                persist(&dir, &board, &format!("kanban: {} volta a feito", card.id))?;
                return Err(error(status, &message));
            }
        }
    };

    let checked = run_check(&state, &session, &scratch).await;
    remove_scratch(&dir, &scratch);

    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    // A sessão do QA acabou no meio (o `load` soltou o card): a `target` não anda.
    let held = board.get(&card.id).is_some_and(|c| {
        c.column == Column::Validando && c.holder.as_deref() == Some(session.id.as_str())
    });
    let landed = match checked {
        Ok(()) if !held => Err((
            StatusCode::CONFLICT,
            "a tarefa saiu do QA durante o check. Nada mudou.".to_owned(),
        )),
        Ok(()) => {
            let (dir, target, scratch) = (dir.clone(), target.clone(), scratch.clone());
            let message = format!("Junta {}: {} (QA)", card.id, card.title);
            tokio::task::spawn_blocking(move || land(&dir, &target, &landing, &scratch, &message))
                .await
                .map_err(|e| internal(&e.to_string()))?
        }
        Err(report) => Err((StatusCode::CONFLICT, report)),
    };
    match landed {
        Ok(commit) => validated(&mut board, &dir, &card.id, &session.id, &target, commit),
        Err((status, message)) => {
            if board.check_failed(&card.id, &session.id).is_ok() {
                persist(&dir, &board, &format!("kanban: {} volta a feito", card.id))?;
            }
            Err(error(status, &message))
        }
    }
}

/// Card juntado em `target` no `commit`: vai para "validado".
fn validated(
    board: &mut Board,
    dir: &Path,
    id: &str,
    holder: &str,
    target: &str,
    commit: String,
) -> Reply<Json<Card>> {
    let merged = Merged {
        branch: target.to_owned(),
        commit,
    };
    let card = board
        .validate(id, holder, merged)
        .map_err(kanban_error)?
        .clone();
    persist(
        dir,
        board,
        &format!("kanban: {id} validada e juntada em {target}"),
    )?;
    Ok(Json(card))
}

/// Roda o check do projeto em `cwd` num pedido `host` (F9.9): você aprova no app, que roda no PC
/// e devolve o resultado. Nunca "sempre permitido". `Err` = o que dizer ao QA.
async fn run_check(state: &AppState, session: &SessionInfo, cwd: &Path) -> Result<(), String> {
    // O comando vem da engrenagem do projeto, nunca do `.assistente.toml` (o agente o edita).
    let command = project_settings_for(&state.0, &session.profile, &session.project).check;
    // Env do perfil como na aba Terminal (sem token: o comando roda fora do jail).
    let spec = SessionSpec {
        profile: session.profile.clone(),
        project: session.project.clone(),
        agent: AgentKind::Shell,
    };
    let plan = LaunchPlan::build(&state.0.config, &state.0.home, &spec, 1)
        .map_err(|e| format!("não consegui montar o check: {e}"))?;
    let (view, _) = state
        .0
        .exec
        .submit(NewRequest {
            session: session.id.clone(),
            profile: session.profile.clone(),
            project: session.project.clone(),
            cwd: cwd.to_owned(),
            command: command.clone(),
            target: ExecTarget::Host,
            env: plan.env,
            timeout_secs: Some(CHECK_TIMEOUT.as_secs()),
            auto_approved: false,
        })
        .map_err(|e| format!("não consegui pedir o check ({e:?}): aprove ou negue os pedidos pendentes e valide de novo"))?;
    tracing::info!("check {} de {} no PC: {command}", view.id, session.id);
    // O `wait` acorda nos prazos do pedido (aprovação, timeout no PC): repetir até o fim.
    let view = loop {
        match state.0.exec.wait(&view.id, Duration::from_secs(60)).await {
            Some(view) if view.state.is_final() => break view,
            Some(_) => continue,
            None => {
                return Err(
                    "o pedido do check sumiu (o hubd reiniciou?): valide de novo".to_owned(),
                );
            }
        }
    };
    check_verdict(&view)
}

/// Verde só com código 0 no tempo. Senão, o motivo e o fim da saída.
fn check_verdict(view: &ExecView) -> Result<(), String> {
    let result = view.result.as_ref();
    let why = match view.state {
        exec::ExecState::Done => match result {
            Some(r) if r.timed_out => "passou do tempo".to_owned(),
            Some(r) if r.exit_code == Some(0) => return Ok(()),
            Some(r) => match r.exit_code {
                Some(code) => format!("saiu com {code}"),
                None => "morreu por sinal".to_owned(),
            },
            None => "terminou sem resultado".to_owned(),
        },
        exec::ExecState::Denied => "foi negado no app".to_owned(),
        exec::ExecState::Expired => "expirou sem resposta do app".to_owned(),
        _ => "não conseguiu rodar".to_owned(),
    };
    let output = result
        .map(|r| tail(&format!("{}{}", r.stdout, r.stderr)))
        .unwrap_or_default();
    Err(format!(
        "o check `{}` {why}. Nada mudou: o card voltou para \"feito\".\n{output}",
        view.command
    )
    .trim_end()
    .to_owned())
}

/// As últimas linhas de `text`, no máximo `CHECK_TAIL_BYTES`.
fn tail(text: &str) -> String {
    let text = text.trim_end();
    let lines: Vec<&str> = text.lines().collect();
    let mut out = lines[lines.len().saturating_sub(CHECK_TAIL_LINES)..].join("\n");
    if out.len() > CHECK_TAIL_BYTES {
        let mut start = out.len() - CHECK_TAIL_BYTES;
        while !out.is_char_boundary(start) {
            start += 1;
        }
        out = out[start..].to_owned();
    }
    out
}

/// Ponta da `target` se ela já contém `source`: a tarefa já foi juntada.
fn already_in(dir: &Path, source: &str, target: &str) -> Option<String> {
    let commit = |rev: &str| {
        git_out(
            dir,
            &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
        )
    };
    let tip = commit(source).ok()?;
    let old = commit(&format!("refs/heads/{target}")).ok()?;
    git_out(dir, &["merge-base", "--is-ancestor", &tip, &old])
        .is_ok()
        .then_some(old)
}

/// O que a `target` vai receber (F11.17): `new` (o merge, ou `tip` no fast-forward), em checkout
/// na worktree descartável para o check. `old` é a `target` de agora; `None` = ela nasce em `new`.
#[derive(Debug)]
struct Landing {
    old: Option<String>,
    new: String,
    tip: String,
}

/// Monta em `scratch` o commit que a `target` vai receber de `source` (a branch da tarefa, ou o
/// commit dela). Não mexe na `target`. Conflito ou worktree da tarefa suja = 409.
fn prepare_merge(
    dir: &Path,
    worktree: &Path,
    source: &str,
    target: &str,
    scratch: &Path,
    message: &str,
) -> Result<Landing, (StatusCode, String)> {
    let conflict = |message: String| (StatusCode::CONFLICT, message);
    if worktree.exists()
        && !git_out(worktree, &["status", "--porcelain"])
            .map_err(conflict)?
            .is_empty()
    {
        return Err(conflict(
            "o worktree tem mudança sem commit: commite (ou descarte) e valide de novo".to_owned(),
        ));
    }
    let tip = git_out(
        dir,
        &["rev-parse", "--verify", &format!("{source}^{{commit}}")],
    )
    .map_err(|_| conflict(format!("{source} não existe")))?;
    let old = git_out(
        dir,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{target}^{{commit}}"),
        ],
    )
    .ok();
    let fast_forward = old
        .as_deref()
        .is_none_or(|old| git_out(dir, &["merge-base", "--is-ancestor", old, &tip]).is_ok());
    let new = if fast_forward {
        add_scratch(dir, scratch, &tip).map_err(|e| {
            remove_scratch(dir, scratch);
            conflict(format!("não consegui montar a worktree do check: {e}"))
        })?;
        tip.clone()
    } else {
        let old = old.as_deref().unwrap_or_default();
        merge_in_scratch(dir, scratch, old, &tip, message).map_err(|e| {
            remove_scratch(dir, scratch);
            let name = source.trim_start_matches("refs/heads/");
            conflict(format!(
                "conflito ao juntar {name} em {target} ({e}). Nada mudou: rode `git merge \
                 {target}` no worktree, resolva, commite e valide de novo"
            ))
        })?
    };
    Ok(Landing { old, new, tip })
}

/// Anda a `target` para o commit verificado, pela regra da F8.8: fast-forward se ela é o
/// checkout do projeto, senão troca a ref conferindo o valor antigo. Se só o quadro andou nela
/// durante o check (os commits do kanban vão para o checkout), junta a tarefa de novo por cima,
/// em `scratch`: o código é o que o check viu. Outra mudança = 409, nada muda.
fn land(
    dir: &Path,
    target: &str,
    landing: &Landing,
    scratch: &Path,
    message: &str,
) -> Result<String, (StatusCode, String)> {
    let conflict = |message: String| (StatusCode::CONFLICT, message);
    let moved_away = || {
        conflict(format!(
            "{target} andou durante o check. Nada mudou: valide de novo"
        ))
    };
    let target_ref = format!("refs/heads/{target}");
    let Some(old) = &landing.old else {
        git_out(dir, &["branch", target, &landing.new]).map_err(|_| moved_away())?;
        return Ok(landing.new.clone());
    };
    let current = git_out(
        dir,
        &["rev-parse", "--verify", &format!("{target_ref}^{{commit}}")],
    )
    .map_err(|_| moved_away())?;
    let new = if current == *old {
        landing.new.clone()
    } else {
        let changed = git_out(dir, &["diff", "--name-only", old, &current]).map_err(conflict)?;
        if changed.lines().any(|path| path != hub_core::kanban::FILE) {
            return Err(moved_away());
        }
        let merged = merge_in_scratch(dir, scratch, &current, &landing.tip, message);
        remove_scratch(dir, scratch);
        merged.map_err(|e| conflict(format!("não consegui juntar de novo em {target} ({e})")))?
    };
    let moved = match checkout_of(dir, &target_ref) {
        Some(checkout) => git_out(&checkout, &["merge", "--ff-only", &new]),
        None => git_out(dir, &["update-ref", &target_ref, &new, &current]),
    };
    moved.map_err(|e| {
        let why = e.lines().next().unwrap_or_default();
        conflict(format!(
            "não consegui avançar {target} ({why}). Nada mudou."
        ))
    })?;
    Ok(new)
}

fn add_scratch(dir: &Path, scratch: &Path, commit: &str) -> Result<(), String> {
    let scratch_arg = scratch.to_string_lossy().into_owned();
    git_out(dir, &["worktree", "add", "--detach", &scratch_arg, commit]).map(drop)
}

fn remove_scratch(dir: &Path, scratch: &Path) {
    let scratch_arg = scratch.to_string_lossy().into_owned();
    let _ = git_out(dir, &["worktree", "remove", "--force", &scratch_arg]);
    let _ = std::fs::remove_dir_all(scratch);
    let _ = git_out(dir, &["worktree", "prune"]);
}

fn merge_in_scratch(
    dir: &Path,
    scratch: &Path,
    old: &str,
    tip: &str,
    message: &str,
) -> Result<String, String> {
    add_scratch(dir, scratch, old)?;
    let mut args = hubd_identity_args(dir);
    args.extend(["merge", "--no-ff", "-m", message, tip].map(str::to_owned));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if let Err(e) = git_out(scratch, &args) {
        let _ = git_out(scratch, &["merge", "--abort"]);
        return Err(e.lines().next().unwrap_or_default().to_owned());
    }
    git_out(scratch, &["rev-parse", "HEAD"])
}

/// Pasta onde `target_ref` está em checkout (o projeto ou um worktree), se alguma.
fn checkout_of(dir: &Path, target_ref: &str) -> Option<PathBuf> {
    let list = git_out(dir, &["worktree", "list", "--porcelain"]).ok()?;
    let mut path = None;
    for line in list.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if line.strip_prefix("branch ") == Some(target_ref) {
            return path;
        }
    }
    None
}

#[derive(Deserialize)]
pub(super) struct ReopenBody {
    id: String,
    note: String,
}

/// POST /v1/kanban/reopen `{id, note}` (F11.15): o QA reprovou. A tarefa volta ao backlog com
/// "Revisão (data): nota" na descrição, e o próximo trabalhador continua da mesma branch.
pub(super) async fn agent_reopen(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ReopenBody>,
) -> Reply<Json<Card>> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let date = sleep::today()
        .await
        .map_err(|e| internal(&format!("{e:#}")))?;
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    let card = board
        .reopen(&body.id, &session.id, &body.note, &date)
        .map_err(kanban_error)?
        .clone();
    persist(
        &dir,
        &board,
        &format!("kanban: {} reprovada no QA; volta ao backlog", card.id),
    )?;
    Ok(Json(card))
}

#[derive(Deserialize)]
pub(super) struct MergeMainBody {
    commit: String,
    /// As tarefas que o commit junta; vazio = só o que a revisão mudou (as notas do backlog).
    #[serde(default)]
    ids: Vec<String>,
}

/// Branch que a revisão (F11.12) leva para a frente.
const REVIEW_TARGET: &str = "main";

/// POST /v1/kanban/juntar `{commit, ids}` (F11.12): a revisão juntou as tarefas `ids` numa
/// worktree descartável e o check passou. O hubd anda com a `main` até `commit` pela regra da F8.8
/// (só para a frente; nunca push) e as tarefas vão para "validado" com `merged`. A `main` andou
/// desde então, ou uma tarefa não está no commit: 409, nada muda.
pub(super) async fn agent_merge_main(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<MergeMainBody>,
) -> Reply<Json<Vec<Card>>> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let Some(commit) = checked_commit(Some(body.commit))? else {
        return Err(error(StatusCode::BAD_REQUEST, "commit inválido"));
    };
    if let Some(id) = body.ids.iter().find(|id| !worker::valid_task_id(id)) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            &format!("id de tarefa inválido: {id}"),
        ));
    }
    let _guard = state.0.kanban_lock.lock().await;
    let mut board = load(&state, &dir).await?;
    // Confere quem segura antes de mexer no git.
    let mut sources = Vec::new();
    for id in &body.ids {
        let card = board
            .get(id)
            .ok_or_else(|| kanban_error(KanbanError::NotFound))?;
        if card.column != Column::Feito {
            return Err(kanban_error(KanbanError::NotDone(card.column)));
        }
        if card.holder.as_deref() != Some(session.id.as_str()) {
            return Err(kanban_error(KanbanError::NotHolder));
        }
        let branch = format!("refs/heads/{}", worker::branch(id));
        let source = match &card.commit {
            Some(commit)
                if git_out(&dir, &["rev-parse", "--verify", "--quiet", &branch]).is_err() =>
            {
                commit.clone()
            }
            _ => branch,
        };
        sources.push((id.clone(), source));
    }
    let merged = {
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || advance_main(&dir, &commit, &sources))
            .await
            .map_err(|e| internal(&e.to_string()))?
            .map_err(|(status, message)| error(status, &message))?
    };
    let mut cards = Vec::new();
    for id in &body.ids {
        let card = board
            .validate(
                id,
                &session.id,
                Merged {
                    branch: REVIEW_TARGET.to_owned(),
                    commit: merged.clone(),
                },
            )
            .map_err(kanban_error)?;
        cards.push(card.clone());
    }
    if !cards.is_empty() {
        persist(
            &dir,
            &board,
            &format!(
                "kanban: {} na {REVIEW_TARGET} (revisão)",
                body.ids.join(", ")
            ),
        )?;
    }
    Ok(Json(cards))
}

/// Leva a `main` até `commit`, só para a frente, conferindo antes que cada tarefa de `sources`
/// (`(id, branch ou commit)`) está nele. Quem tem a `main` em checkout anda com `merge --ff-only`;
/// senão a ref troca conferindo o valor antigo (F8.8). Devolve o commit da `main` depois.
fn advance_main(
    dir: &Path,
    commit: &str,
    sources: &[(String, String)],
) -> Result<String, (StatusCode, String)> {
    let conflict = |message: String| (StatusCode::CONFLICT, message);
    let new = git_out(
        dir,
        &["rev-parse", "--verify", &format!("{commit}^{{commit}}")],
    )
    .map_err(|_| conflict(format!("o commit {commit} não existe no projeto")))?;
    let target_ref = format!("refs/heads/{REVIEW_TARGET}");
    let old = git_out(
        dir,
        &["rev-parse", "--verify", &format!("{target_ref}^{{commit}}")],
    )
    .map_err(|_| conflict(format!("o projeto não tem a branch {REVIEW_TARGET}")))?;
    let ancestor = |a: &str, b: &str| git_out(dir, &["merge-base", "--is-ancestor", a, b]).is_ok();
    for (id, source) in sources {
        if !ancestor(source, &new) {
            return Err(conflict(format!(
                "{id} não está no commit {commit}: junte a branch dela antes. Nada mudou."
            )));
        }
    }
    if ancestor(&new, &old) {
        return Ok(old);
    }
    if !ancestor(&old, &new) {
        return Err(conflict(format!(
            "a {REVIEW_TARGET} andou desde o começo da revisão (está em {}). Nada mudou.",
            &old[..old.len().min(7)]
        )));
    }
    let moved = match checkout_of(dir, &target_ref) {
        Some(checkout) => git_out(&checkout, &["merge", "--ff-only", &new]),
        None => git_out(dir, &["update-ref", &target_ref, &new, &old]),
    };
    moved.map_err(|e| {
        let why = e.lines().next().unwrap_or_default();
        conflict(format!(
            "não consegui avançar {REVIEW_TARGET} ({why}). Nada mudou."
        ))
    })?;
    Ok(new)
}

/// Pasta dos resumos das revisões (F11.12), relativa ao projeto, fora do git (`info/exclude`).
const REVIEWS_DIR: &str = ".assistente/revisoes";
/// Tamanho máximo de um resumo, em caracteres.
const MAX_SUMMARY: usize = 20_000;

/// Último resumo de revisão do projeto, mostrado no quadro.
#[derive(Debug, Clone, Serialize)]
pub(super) struct ReviewSummary {
    /// Caminho relativo ao projeto (`.assistente/revisoes/<data-hora>.md`).
    file: String,
    text: String,
}

#[derive(Deserialize)]
pub(super) struct SummaryBody {
    text: String,
}

/// POST /v1/kanban/resumo `{text}` (F11.12): o resumo da revisão (o que entrou, o que voltou e
/// por quê, ou onde parou) vai para `.assistente/revisoes/<data-hora>.md` e aparece no quadro.
pub(super) async fn agent_review_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SummaryBody>,
) -> Reply<(StatusCode, Json<ReviewSummary>)> {
    let session = agent_session(&state, &headers).await?;
    let dir = project_dir(&state, &session.profile, &session.project)?;
    let text = body.text.trim().to_owned();
    if text.is_empty() {
        return Err(error(StatusCode::BAD_REQUEST, "o resumo está vazio"));
    }
    if text.chars().count() > MAX_SUMMARY {
        return Err(error(
            StatusCode::BAD_REQUEST,
            &format!("resumo com mais de {MAX_SUMMARY} caracteres"),
        ));
    }
    let stamp = timestamp().await?;
    let folder = dir.join(REVIEWS_DIR);
    std::fs::create_dir_all(&folder).map_err(|e| internal(&e.to_string()))?;
    crate::sleep::exclude_line(&dir, worker::REVIEWS_EXCLUDE)
        .await
        .map_err(|e| internal(&format!("{e:#}")))?;
    let name = format!("{stamp}.md");
    std::fs::write(folder.join(&name), format!("{text}\n"))
        .map_err(|e| internal(&e.to_string()))?;
    tracing::info!(
        "resumo da revisão de {} em {REVIEWS_DIR}/{name}",
        session.id
    );
    Ok((
        StatusCode::CREATED,
        Json(ReviewSummary {
            file: format!("{REVIEWS_DIR}/{name}"),
            text,
        }),
    ))
}

/// Data e hora locais para o nome do resumo (`2026-10-22-153400`).
async fn timestamp() -> Reply<String> {
    let output = tokio::process::Command::new("date")
        .arg("+%F-%H%M%S")
        .output()
        .await
        .map_err(|e| internal(&format!("date: {e}")))?;
    let stamp = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if stamp.is_empty() || !stamp.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return Err(internal("date devolveu uma data estranha"));
    }
    Ok(stamp)
}

/// O resumo mais novo de `.assistente/revisoes` (os nomes são a data e hora), se houver.
fn latest_review(dir: &Path) -> Option<ReviewSummary> {
    let name = std::fs::read_dir(dir.join(REVIEWS_DIR))
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.ends_with(".md"))
        .max()?;
    let text = std::fs::read_to_string(dir.join(REVIEWS_DIR).join(&name)).ok()?;
    Some(ReviewSummary {
        file: format!("{REVIEWS_DIR}/{name}"),
        text: text.trim().chars().take(MAX_SUMMARY).collect(),
    })
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

/// Recolhe trabalhadores mortos do projeto (F14.5): para cada sessão de trabalhador (opencode)
/// que está morta, guarda no card o status de saída e as últimas ~50 linhas da saída (cortadas
/// em 4 KiB) no campo `last_error`, incrementa `failures` e encerra a sessão.
/// Sessões do Claude ou OpenCode interativo nunca são recolhidas.
async fn collect_failed_workers(
    state: &AppState,
    profile: &str,
    project: &str,
    dir: &Path,
) -> Result<(), String> {
    // Lista todas as sessões do projeto (incluindo mortas)
    let all_sessions = state.0.tmux.list().await.map_err(|e| e.to_string())?;
    let project_sessions: Vec<_> = all_sessions
        .into_iter()
        .filter(|s| s.profile == profile && s.project == project && s.agent == AgentKind::Opencode)
        .collect();

    if project_sessions.is_empty() {
        return Ok(());
    }

    let mut board = load(state, dir).await?;

    for session in project_sessions {
        if !session.dead {
            continue; // Só recolhe sessões mortas
        }

        // Verifica se esta sessão segura algum card em "fazendo"
        let card_id = board
            .cards
            .iter()
            .find(|c| c.column == Column::Fazendo && c.holder.as_deref() == Some(session.id.as_str()))
            .map(|c| c.id.clone());

        let Some(card_id) = card_id else {
            // Sessão morta mas não segura nenhuma tarefa: só encerra
            let _ = state.0.tmux.kill(&session.id).await;
            continue;
        }

        // Captura o estado do painel (status de saída)
        let pane_state = state.0.tmux.pane_state(&session.id).await;
        let exit_status = match pane_state {
            PaneState::Dead(Some(code)) => format!("exit {}", code),
            PaneState::Dead(None) => "sinal (sem código de saída)".to_owned(),
            PaneState::Alive => continue, // Não deveria acontecer, mas por segurança
        };

        // Captura as últimas ~50 linhas da saída (cortadas em 4 KiB)
        let output = match state.0.tmux.capture_pane(&session.id, -50).await {
            Ok(text) => {
                // Limita a 4 KiB
                const MAX_BYTES: usize = 4096;
                let bytes = text.as_bytes();
                if bytes.len() > MAX_BYTES {
                    let truncated = String::from_utf8_lossy(&bytes[..MAX_BYTES]);
                    format!("{}… (cortado em 4 KiB)", truncated)
                } else {
                    text
                }
            }
            Err(e) => format!("erro ao capturar saída: {e}"),
        };

        // Monta a mensagem de erro
        let error_msg = format!("{exit_status}\n\n{output}");

        // Atualiza o card
        if let Some(card) = board.get_mut(&card_id) {
            card.last_error = Some(error_msg);
            card.failures += 1;
            card.column = Column::Backlog;
            card.holder = None;
        }

        // Encerra a sessão
        let _ = state.0.tmux.kill(&session.id).await;

        tracing::info!("orquestrador: recolheu trabalhador {} da tarefa {}", session.id, card_id);
    }

    // Salva o quadro se houve mudanças
    let has_changes = board.cards.iter().any(|c| c.last_error.is_some());
    if has_changes {
        store::save(dir, &board, "kanban: recolhe trabalhadores mortos").map_err(|e| e.to_string())?;
    }

    Ok(())
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
    // Recolhe trabalhadores mortos antes de abrir novos (F14.5)
    collect_failed_workers(state, profile, project, &dir).await?;
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
            // Pausado por cota (F11.14): nada abre até "Retomar"; não é erro do laço.
            if !board.orchestrator.enabled
                || board.orchestrator.paused.is_some()
                || board.in_progress() >= limit
            {
                break;
            }
            match board.next_auto(landed(&dir)) {
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
    if started.is_empty() {
        auto_review(state, &dir, profile, project).await?;
    }
    Ok(started)
}

/// Revisão automática (F11.12): com o orquestrador ligado, sem pausa, nada em "fazendo" e
/// nenhum card "feito" já em revisão ou QA, os "feitos" livres vão para uma revisão. Os mesmos
/// cards (mesmo commit) não vão duas vezes: o que a revisão não tocou espera você.
async fn auto_review(
    state: &AppState,
    dir: &Path,
    profile: &str,
    project: &str,
) -> Result<(), String> {
    let pending = {
        let _guard = state.0.kanban_lock.lock().await;
        let board = match load(state, dir).await {
            Ok(board) => board,
            Err(response) => return Err(reply_text(response).await),
        };
        let done = || {
            board
                .cards
                .iter()
                .filter(|card| card.column == Column::Feito)
        };
        if !board.orchestrator.enabled
            || board.orchestrator.paused.is_some()
            || board.in_progress() > 0
            || done().any(|card| card.holder.is_some())
        {
            return Ok(());
        }
        let mut pending: Vec<String> = done()
            .map(|card| format!("{}@{}", card.id, card.commit.as_deref().unwrap_or("")))
            .collect();
        pending.sort();
        pending.join(" ")
    };
    let key = orchestrator_key(profile, project);
    if pending.is_empty()
        || state
            .0
            .auto_reviews
            .lock()
            .expect("lock das revisões")
            .get(&key)
            == Some(&pending)
    {
        return Ok(());
    }
    match hold_done_cards(state, profile, project, true).await {
        Ok(info) => {
            tracing::info!("orquestrador: {key} revisão com {}", info.id);
            state
                .0
                .auto_reviews
                .lock()
                .expect("lock das revisões")
                .insert(key, pending);
            Ok(())
        }
        Err(response) => Err(format!("revisão: {}", reply_text(response).await)),
    }
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

    /// O app fingido (F11.17): espera o pedido `host` do check (pendente) e o devolve.
    async fn pending_check(app: Router) -> serde_json::Value {
        loop {
            let (_, list) = send(app.clone(), "GET", "/api/exec", None, None).await;
            let pending = list
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["state"] == "pending");
            if let Some(view) = pending {
                return view.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Responde o pedido do check como o app: `None` nega; `Some(código)` aprova e devolve o
    /// resultado do PC.
    async fn finish_check(app: Router, view: &serde_json::Value, code: Option<i32>) {
        let id = view["id"].as_str().unwrap();
        let Some(code) = code else {
            let (status, _) = send(app, "POST", &format!("/api/exec/{id}/deny"), None, None).await;
            assert_eq!(status, StatusCode::NO_CONTENT);
            return;
        };
        let uri = format!("/api/exec/{id}/approve");
        let (status, _) = send(app.clone(), "POST", &uri, Some("{}"), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        // O app pega o pedido antes de rodar (F9.11): só pedido pego aceita resultado.
        let uri = format!("/api/exec/{id}/claim");
        let (status, _) = send(app.clone(), "POST", &uri, None, None).await;
        assert_eq!(status, StatusCode::OK);
        let result = json!({
            "exit_code": code,
            "stdout": "linha 1\nteste quebrado: falta o botão\n",
        });
        let uri = format!("/api/exec/{id}/result");
        let (status, _) = send(app, "POST", &uri, Some(&result.to_string()), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    async fn answer_check(app: Router, code: Option<i32>) -> serde_json::Value {
        let view = pending_check(app.clone()).await;
        finish_check(app, &view, code).await;
        view
    }

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
    async fn quota_pauses_the_orchestrator_until_resumed_and_claude_finishes_the_card() {
        let t = TestTmux::new("kanban-cota");
        let hub = hub(&t, true);
        std::fs::write(
            hub.proj.join(".assistente.toml"),
            "[workers]\nmodels = [\"x/a\"]\n",
        )
        .unwrap();
        git_sync(&hub.proj, &["add", ".assistente.toml"]);
        git_sync(&hub.proj, &["commit", "-q", "-m", "config do projeto"]);
        for title in ["um", "dois", "tres"] {
            let body = format!(r#"{{"title":"{title}","kind":"agente"}}"#);
            send(hub.app.clone(), "POST", CARDS, Some(&body), None).await;
        }
        let card = |board: &serde_json::Value, id: &str| -> serde_json::Value {
            board["cards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap()
                .clone()
        };
        let uri = format!("{CARDS}/orquestrador");
        let (_, board) = send(
            hub.app.clone(),
            "PUT",
            &uri,
            Some(r#"{"enabled":true,"max_workers":1}"#),
            None,
        )
        .await;
        let worker = card(&board, "T1")["holder"].as_str().unwrap().to_owned();
        let token = exec_token(&t, &worker).await;

        // A cota acaba: T1 volta sem falha, com o motivo, e o orquestrador pausa.
        let (status, blocked) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/block",
            Some(r#"{"id":"T1","reason":"Error: Free usage limit reached."}"#),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{blocked}");
        assert_eq!(blocked["column"], "backlog");
        assert_eq!(blocked["blocked"], "Error: Free usage limit reached.");
        assert!(blocked.get("failures").is_none(), "{blocked}");
        assert_eq!(
            last_subject(&hub.proj),
            "kanban: T1 parada por cota; orquestrador pausado"
        );
        t.tmux.kill(&worker).await.unwrap();
        // Com vaga e pausado, uma volta não abre nada (e não é erro).
        let (_, board) = send(
            hub.app.clone(),
            "PUT",
            &uri,
            Some(r#"{"enabled":true,"max_workers":2}"#),
            None,
        )
        .await;
        assert!(board.get("started").is_none(), "{board}");
        assert!(board.get("orchestrator_error").is_none(), "{board}");
        assert_eq!(
            board["orchestrator"]["paused"]["reason"],
            "Error: Free usage limit reached."
        );
        for id in ["T1", "T2", "T3"] {
            assert_eq!(card(&board, id)["column"], "backlog", "{board}");
        }

        // "Terminar com… Claude Code": um Claude no worktree da T1 segura o card.
        let (status, info) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T1/claude"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        assert_eq!(info["agent"], "claude");
        let claude = info["id"].as_str().unwrap().to_owned();
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        let t1 = card(&board, "T1");
        assert_eq!(t1["column"], "fazendo");
        assert_eq!(t1["holder"], claude.as_str());
        assert!(t1.get("blocked").is_none(), "{t1}");
        assert!(worker::worktree_dir(&hub.proj, "T1").join(".git").exists());
        // Ele mesmo marca a tarefa, pela API dos agentes.
        let token = exec_token(&t, &claude).await;
        let (status, done) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/done",
            Some(r#"{"id":"T1","commit":"abcdef1"}"#),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{done}");
        let (status, _) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T1/claude"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "só card do backlog");

        // "Retomar" tira a pausa e já abre o próximo.
        let (status, board) = send(
            hub.app.clone(),
            "POST",
            &format!("{uri}/retomar"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{board}");
        assert!(board["orchestrator"].get("paused").is_none(), "{board}");
        assert_eq!(board["started"], json!(["T2", "T3"]), "{board}");
    }

    #[tokio::test]
    async fn qa_merges_into_the_chosen_branch_or_reopens_with_what_is_missing() {
        let t = TestTmux::new("kanban-qa");
        let hub = hub(&t, true);
        for title in ["um", "dois", "tres"] {
            let body = format!(r#"{{"title":"{title}","kind":"agente"}}"#);
            send(hub.app.clone(), "POST", CARDS, Some(&body), None).await;
        }
        // Um trabalhador fez as três: T1 e T3 com commit no worktree, T3 em conflito com a main.
        let (_, worker) = session(&t, AgentKind::Opencode, 1).await;
        for id in ["T1", "T2", "T3"] {
            send(
                hub.agents.clone(),
                "POST",
                "/v1/kanban/pull",
                None,
                Some(&worker),
            )
            .await;
            let dir = worker::worktree_dir(&hub.proj, id);
            let path = dir.display().to_string();
            let branch = worker::branch(id);
            git_sync(
                &hub.proj,
                &["worktree", "add", "-q", "-b", &branch, &path, "HEAD"],
            );
        }
        let t1 = worker::worktree_dir(&hub.proj, "T1");
        let t3 = worker::worktree_dir(&hub.proj, "T3");
        crate::sleep::tests::commit(&t1, "b.txt", "feito por T1");
        // O agente edita o `.assistente.toml`: um `check` lá não vale (F11.17).
        crate::sleep::tests::commit(&t1, ".assistente.toml", "check = \"true\"\n");
        crate::sleep::tests::commit(&t3, "c.txt", "T3");
        crate::sleep::tests::commit(&hub.proj, "c.txt", "main");
        for id in ["T1", "T2", "T3"] {
            let body = format!(r#"{{"id":"{id}"}}"#);
            let (status, _) = send(
                hub.agents.clone(),
                "POST",
                "/v1/kanban/done",
                Some(&body),
                Some(&worker),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }
        let card = |board: &serde_json::Value, id: &str| -> serde_json::Value {
            board["cards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap()
                .clone()
        };
        let qa = |id: &str| {
            let app = hub.app.clone();
            let uri = format!("{CARDS}/{id}/qa");
            async move { send(app, "POST", &uri, None, None).await }
        };
        let validate = |id: &str, branch: &str, token: &str| {
            let agents = hub.agents.clone();
            let body = format!(r#"{{"id":"{id}","branch":"{branch}"}}"#);
            let token = token.to_owned();
            async move {
                send(
                    agents,
                    "POST",
                    "/v1/kanban/validate",
                    Some(&body),
                    Some(&token),
                )
                .await
            }
        };

        // QA da T1: um Claude no worktree segura o card; um segundo QA não abre.
        let (status, info) = qa("T1").await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        assert_eq!(info["agent"], "claude");
        let qa1 = info["id"].as_str().unwrap().to_owned();
        let token1 = exec_token(&t, &qa1).await;
        assert_eq!(qa("T1").await.0, StatusCode::CONFLICT);
        let (status, _) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T1/claude"),
            None,
            None,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "card feito não volta a ser feito"
        );
        // Só quem faz o QA valida; branch esquisita e árvore suja não passam.
        assert_eq!(
            validate("T1", "main", &worker).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(
            validate("T1", "-x", &token1).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            validate("T1", "agente/T1", &token1).await.0,
            StatusCode::BAD_REQUEST
        );
        std::fs::write(t1.join("sujo.txt"), "x").unwrap();
        let (status, body) = validate("T1", "main", &token1).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        std::fs::remove_file(t1.join("sujo.txt")).unwrap();
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(card(&board, "T1")["column"], "feito");
        // Check vermelho ou negado: a main não anda, o QA vê o fim da saída e o card volta.
        let (red, view) = tokio::join!(
            validate("T1", "main", &token1),
            answer_check(hub.app.clone(), Some(1))
        );
        let (status, body) = red;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        let message = body["error"].as_str().unwrap();
        assert!(message.contains("saiu com 1"), "{message}");
        assert!(message.contains("falta o botão"), "{message}");
        // O comando é o da engrenagem (padrão), nunca o do `.assistente.toml`, e roda no PC, no
        // merge descartável dentro do projeto, que some depois.
        assert_eq!(view["command"], "./dev just check");
        assert_eq!(view["target"], "host");
        let scratch = PathBuf::from(view["cwd"].as_str().unwrap());
        assert!(
            scratch.starts_with(hub.proj.join(".assistente/worktrees")),
            "{view}"
        );
        assert!(!scratch.exists());
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(card(&board, "T1")["column"], "feito");
        assert_eq!(card(&board, "T1")["holder"], qa1.as_str());
        let (denied, _) = tokio::join!(
            validate("T1", "main", &token1),
            answer_check(hub.app.clone(), None)
        );
        assert_eq!(denied.0, StatusCode::CONFLICT, "{}", denied.1);
        assert!(
            git_out(
                &hub.proj,
                &["merge-base", "--is-ancestor", "agente/T1", "main"]
            )
            .is_err(),
            "a main não recebeu a T1"
        );
        // Verde: a main (o checkout do projeto) anda com o merge, sem mexer no que é seu. Enquanto
        // o check roda, o card fica "validando".
        let (green, _) = tokio::join!(validate("T1", "main", &token1), async {
            let view = pending_check(hub.app.clone()).await;
            let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
            assert_eq!(card(&board, "T1")["column"], "validando");
            // O quadro anda na main durante o check: a tarefa entra por cima mesmo assim.
            let body = Some(r#"{"title":"durante o check"}"#);
            let (status, _) = send(hub.app.clone(), "POST", CARDS, body, None).await;
            assert_eq!(status, StatusCode::CREATED);
            finish_check(hub.app.clone(), &view, Some(0)).await;
        });
        let (status, done) = green;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert_eq!(done["column"], "validado");
        assert_eq!(done["merged"]["branch"], "main");
        let merged = done["merged"]["commit"].as_str().unwrap().to_owned();
        git_sync(&hub.proj, &["merge-base", "--is-ancestor", &merged, "main"]);
        git_sync(
            &hub.proj,
            &["merge-base", "--is-ancestor", "agente/T1", "main"],
        );
        assert_eq!(
            std::fs::read_to_string(hub.proj.join("b.txt")).unwrap(),
            "feito por T1"
        );
        assert_eq!(
            last_subject(&hub.proj),
            "kanban: T1 validada e juntada em main"
        );
        assert!(git_sync(&hub.proj, &["status", "--porcelain"]).is_empty());

        // T3 em conflito: nada muda e o QA sabe o que fazer.
        let (_, info) = qa("T3").await;
        let token3 = exec_token(&t, info["id"].as_str().unwrap()).await;
        let main_before = git_sync(&hub.proj, &["rev-parse", "main"]);
        let (status, body) = validate("T3", "main", &token3).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains("git merge main"),
            "{body}"
        );
        // A main só ganha os commits do quadro ("validando" e de volta a "feito"): mesma árvore.
        assert_eq!(git_sync(&hub.proj, &["diff", &main_before, "main"]), "");
        // Para outra branch que não existe: ela nasce na ponta da tarefa.
        let ((status, done), _) = tokio::join!(
            validate("T3", "release", &token3),
            answer_check(hub.app.clone(), Some(0))
        );
        assert_eq!(status, StatusCode::OK, "{done}");
        assert_eq!(
            git_sync(&hub.proj, &["rev-parse", "release"]),
            git_sync(&hub.proj, &["rev-parse", "agente/T3"])
        );

        // T2 reprovada: volta ao backlog com o que falta, sem falha.
        let (_, info) = qa("T2").await;
        let token2 = exec_token(&t, info["id"].as_str().unwrap()).await;
        let (status, back) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/reopen",
            Some(r#"{"id":"T2","note":"falta o botão de sair"}"#),
            Some(&token2),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{back}");
        assert_eq!(back["column"], "backlog");
        let description = back["description"].as_str().unwrap();
        assert!(description.starts_with("Revisão ("), "{description}");
        assert!(
            description.ends_with("): falta o botão de sair"),
            "{description}"
        );
        assert!(back.get("failures").is_none(), "{back}");

        // A sessão do QA acaba: worktree e branch da T1 saem; o card fica validado.
        t.tmux.kill(&qa1).await.unwrap();
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        let t1_card = card(&board, "T1");
        assert_eq!(t1_card["column"], "validado");
        assert!(t1_card.get("holder").is_none(), "{t1_card}");
        assert!(!t1.exists());
        assert!(git_sync(&hub.proj, &["branch", "--list", "agente/T1"]).is_empty());
    }

    #[tokio::test]
    async fn qa_all_holds_every_free_done_card_in_one_session() {
        let t = TestTmux::new("kanban-qa-geral");
        let hub = hub(&t, true);
        let qa_all = || {
            let app = hub.app.clone();
            async move { send(app, "POST", &format!("{CARDS}/qa"), None, None).await }
        };
        for title in ["um", "dois", "tres", "quatro"] {
            let body = format!(r#"{{"title":"{title}","kind":"agente"}}"#);
            send(hub.app.clone(), "POST", CARDS, Some(&body), None).await;
        }
        assert_eq!(qa_all().await.0, StatusCode::CONFLICT, "nada em feito");
        // T1 e T2 feitos sem branch (como no modo dormir), com o commit que já está na main; T3
        // feito e já num QA próprio; T4 fica no backlog.
        let head = git_sync(&hub.proj, &["rev-parse", "HEAD"]);
        let (_, worker) = session(&t, AgentKind::Opencode, 1).await;
        for id in ["T1", "T2", "T3"] {
            send(
                hub.agents.clone(),
                "POST",
                "/v1/kanban/pull",
                None,
                Some(&worker),
            )
            .await;
            let body = format!(r#"{{"id":"{id}","commit":"{head}"}}"#);
            send(
                hub.agents.clone(),
                "POST",
                "/v1/kanban/done",
                Some(&body),
                Some(&worker),
            )
            .await;
        }
        let (status, single) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T3/qa"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{single}");

        let (status, info) = qa_all().await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        assert_eq!(info["agent"], "claude");
        let qa = info["id"].as_str().unwrap().to_owned();
        assert_eq!(
            last_subject(&hub.proj),
            format!("kanban: QA geral de T1, T2 com {qa}")
        );
        // O prompt vai num arquivo da pasta do projeto (o tmux recusa comando grande), fora do git.
        let prompt = hub.proj.join(format!(".assistente/prompts/{qa}.md"));
        let text = std::fs::read_to_string(&prompt).unwrap();
        assert!(text.starts_with("# QA geral do quadro"), "{text}");
        assert!(git_sync(&hub.proj, &["status", "--porcelain"]).is_empty());
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        let holder = |id: &str| {
            board["cards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap()["holder"]
                .clone()
        };
        assert_eq!(holder("T1"), qa.as_str());
        assert_eq!(holder("T2"), qa.as_str());
        assert_eq!(holder("T3"), single["id"]);
        assert!(holder("T4").is_null());
        assert_eq!(qa_all().await.0, StatusCode::CONFLICT, "nada livre");

        // Sem branch, vale o commit do card: já está na main, então só valida.
        let token = exec_token(&t, &qa).await;
        let main_before = git_sync(&hub.proj, &["rev-parse", "main"]);
        let (status, done) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/validate",
            Some(r#"{"id":"T1","branch":"main"}"#),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert_eq!(done["column"], "validado");
        assert_eq!(done["merged"]["commit"], main_before.as_str());
        // O que o QA geral não tocou fica livre quando ele acaba.
        t.tmux.kill(&qa).await.unwrap();
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        let t2 = board["cards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "T2")
            .unwrap();
        assert_eq!(t2["column"], "feito");
        assert!(t2.get("holder").is_none(), "{t2}");
    }

    #[tokio::test]
    async fn review_merges_what_passes_into_main_and_reopens_the_rest() {
        let t = TestTmux::new("kanban-revisao");
        let hub = hub(&t, true);
        let review = || {
            let app = hub.app.clone();
            async move { send(app, "POST", &format!("{CARDS}/revisar"), None, None).await }
        };
        for title in ["um", "dois", "tres"] {
            let body = format!(r#"{{"title":"{title}","kind":"agente"}}"#);
            send(hub.app.clone(), "POST", CARDS, Some(&body), None).await;
        }
        assert_eq!(review().await.0, StatusCode::CONFLICT, "nada em feito");
        // T1 e T2 mexem no mesmo arquivo (se cruzam); T3 não tem código próprio.
        let (_, worker) = session(&t, AgentKind::Opencode, 1).await;
        for id in ["T1", "T2", "T3"] {
            send(
                hub.agents.clone(),
                "POST",
                "/v1/kanban/pull",
                None,
                Some(&worker),
            )
            .await;
            let path = worker::worktree_dir(&hub.proj, id).display().to_string();
            let branch = worker::branch(id);
            git_sync(
                &hub.proj,
                &["worktree", "add", "-q", "-b", &branch, &path, "HEAD"],
            );
        }
        let t1 = worker::worktree_dir(&hub.proj, "T1");
        let t2 = worker::worktree_dir(&hub.proj, "T2");
        crate::sleep::tests::commit(&t1, "lista.txt", "T1");
        crate::sleep::tests::commit(&t2, "lista.txt", "T2");
        for id in ["T1", "T2", "T3"] {
            let body = format!(r#"{{"id":"{id}"}}"#);
            send(
                hub.agents.clone(),
                "POST",
                "/v1/kanban/done",
                Some(&body),
                Some(&worker),
            )
            .await;
        }

        // Um clique: um Claude na raiz segura os três, com o prompt da revisão.
        let (status, info) = review().await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        assert_eq!(info["agent"], "claude");
        let reviewer = info["id"].as_str().unwrap().to_owned();
        assert_eq!(
            last_subject(&hub.proj),
            format!("kanban: revisão de T1, T2, T3 com {reviewer}")
        );
        let prompt = hub.proj.join(format!(".assistente/prompts/{reviewer}.md"));
        let text = std::fs::read_to_string(&prompt).unwrap();
        assert!(text.starts_with("# Revisão do quadro"), "{text}");
        assert!(text.contains("### T3: tres"), "{text}");
        assert_eq!(review().await.0, StatusCode::CONFLICT, "nada livre");
        let token = exec_token(&t, &reviewer).await;
        let merge_main = |body: String, token: String| {
            let agents = hub.agents.clone();
            async move {
                send(
                    agents,
                    "POST",
                    "/v1/kanban/juntar",
                    Some(&body),
                    Some(&token),
                )
                .await
            }
        };

        // T3 sem código próprio volta ao backlog com a nota.
        let (status, back) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/reopen",
            Some(r#"{"id":"T3","note":"a branch não tem commit próprio; falta tudo"}"#),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{back}");
        assert_eq!(back["column"], "backlog");
        assert!(
            back["description"]
                .as_str()
                .unwrap()
                .contains("): a branch não tem commit próprio"),
            "{back}"
        );

        // A junção numa worktree descartável, com o conflito resolvido.
        let scratch = hub.proj.join(".assistente/worktrees/revisao-1");
        let scratch_arg = scratch.display().to_string();
        git_sync(
            &hub.proj,
            &["worktree", "add", "-q", "--detach", &scratch_arg, "main"],
        );
        git_sync(
            &scratch,
            &["merge", "-q", "--no-ff", "-m", "Junta T1", "agente/T1"],
        );
        let conflict = std::process::Command::new("git")
            .current_dir(&scratch)
            .args(["merge", "-q", "--no-ff", "-m", "Junta T2", "agente/T2"])
            .output()
            .unwrap();
        assert!(!conflict.status.success(), "T1 e T2 se cruzam");
        std::fs::write(scratch.join("lista.txt"), "T1\nT2\n").unwrap();
        git_sync(&scratch, &["add", "lista.txt"]);
        git_sync(&scratch, &["commit", "-q", "--no-edit"]);
        let head = git_sync(&scratch, &["rev-parse", "HEAD"]);

        // Quem não segura não junta; tarefa que voltou ou que não está no commit também não.
        let both = format!(r#"{{"commit":"{head}","ids":["T1","T2"]}}"#);
        assert_eq!(
            merge_main(both.clone(), worker.clone()).await.0,
            StatusCode::CONFLICT
        );
        let with_t3 = format!(r#"{{"commit":"{head}","ids":["T1","T3"]}}"#);
        assert_eq!(
            merge_main(with_t3, token.clone()).await.0,
            StatusCode::CONFLICT
        );
        let main_before = git_sync(&hub.proj, &["rev-parse", "main"]);
        let only_t1 = git_sync(&t1, &["rev-parse", "HEAD"]);
        let missing = format!(r#"{{"commit":"{only_t1}","ids":["T1","T2"]}}"#);
        let (status, body) = merge_main(missing, token.clone()).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(
            body["error"].as_str().unwrap().contains("T2 não está"),
            "{body}"
        );

        // A main andou por fora: nada muda, e o revisor sabe por quê.
        let moved = crate::sleep::tests::commit(&hub.proj, "outro.txt", "por fora");
        let (status, body) = merge_main(both.clone(), token.clone()).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(body["error"].as_str().unwrap().contains("andou"), "{body}");
        assert_eq!(git_sync(&hub.proj, &["rev-parse", "main"]), moved);
        assert_ne!(moved, main_before);
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        let column = |board: &serde_json::Value, id: &str| {
            board["cards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap()["column"]
                .clone()
        };
        assert_eq!(column(&board, "T1"), "feito");

        // Refeita sobre a main nova e com o check verde: a main anda até a junção.
        git_sync(&scratch, &["merge", "-q", "--no-edit", "main"]);
        let head = git_sync(&scratch, &["rev-parse", "HEAD"]);
        let both = format!(r#"{{"commit":"{head}","ids":["T1","T2"]}}"#);
        let (status, merged) = merge_main(both, token.clone()).await;
        assert_eq!(status, StatusCode::OK, "{merged}");
        let merged = merged.as_array().unwrap();
        assert_eq!(merged.len(), 2);
        for card in merged {
            assert_eq!(card["column"], "validado");
            assert_eq!(card["merged"]["branch"], "main");
            assert_eq!(card["merged"]["commit"], head.as_str());
        }
        assert_eq!(last_subject(&hub.proj), "kanban: T1, T2 na main (revisão)");
        git_sync(&hub.proj, &["merge-base", "--is-ancestor", &head, "main"]);
        assert_eq!(
            std::fs::read_to_string(hub.proj.join("lista.txt")).unwrap(),
            "T1\nT2\n"
        );

        // O resumo vai para .assistente/revisoes, fora do git, e aparece no quadro.
        let (status, summary) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/resumo",
            Some(r#"{"text":"Entrou na main: T1, T2.\nVoltou: T3, sem código próprio."}"#),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{summary}");
        let file = summary["file"].as_str().unwrap().to_owned();
        assert!(file.starts_with(".assistente/revisoes/"), "{file}");
        assert!(
            std::fs::read_to_string(hub.proj.join(&file))
                .unwrap()
                .contains("Voltou: T3")
        );
        git_sync(&hub.proj, &["worktree", "remove", "--force", &scratch_arg]);
        let status = git_sync(&hub.proj, &["status", "--porcelain"]);
        assert!(status.is_empty(), "{status}");
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(board["review"]["file"], file.as_str());
        assert!(
            board["review"]["text"]
                .as_str()
                .unwrap()
                .starts_with("Entrou na main")
        );

        // A sessão acaba: worktrees e branches das juntadas saem; a T3 fica no backlog.
        t.tmux.kill(&reviewer).await.unwrap();
        let (_, board) = send(hub.app.clone(), "GET", CARDS, None, None).await;
        assert_eq!(column(&board, "T1"), "validado");
        assert_eq!(column(&board, "T3"), "backlog");
        for id in ["T1", "T2"] {
            assert!(!worker::worktree_dir(&hub.proj, id).exists());
            let branch = worker::branch(id);
            assert!(git_sync(&hub.proj, &["branch", "--list", &branch]).is_empty());
        }
    }

    #[tokio::test]
    async fn orchestrator_reviews_done_cards_when_nothing_is_running() {
        let t = TestTmux::new("kanban-revisao-auto");
        let hub = hub(&t, true);
        for title in ["um", "dois"] {
            let body = format!(r#"{{"title":"{title}","kind":"agente"}}"#);
            send(hub.app.clone(), "POST", CARDS, Some(&body), None).await;
        }
        // T1 feito; T2 em fazendo, com quem o segura.
        let (_, worker) = session(&t, AgentKind::Opencode, 1).await;
        send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/pull",
            None,
            Some(&worker),
        )
        .await;
        send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/done",
            Some(r#"{"id":"T1"}"#),
            Some(&worker),
        )
        .await;
        send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/pull",
            None,
            Some(&worker),
        )
        .await;
        let turn = || {
            let app = hub.app.clone();
            async move {
                send(
                    app,
                    "PUT",
                    &format!("{CARDS}/orquestrador"),
                    Some(r#"{"enabled":true,"max_workers":1}"#),
                    None,
                )
                .await
            }
        };
        let holder = |board: &serde_json::Value| board["cards"][0]["holder"].clone();
        // Algo em fazendo: nada de revisão.
        let (status, board) = turn().await;
        assert_eq!(status, StatusCode::OK, "{board}");
        assert!(holder(&board).is_null(), "{board}");
        // O trabalhador acaba: a próxima volta abre a revisão sozinha.
        send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/done",
            Some(r#"{"id":"T2"}"#),
            Some(&worker),
        )
        .await;
        let (_, board) = turn().await;
        assert!(board.get("orchestrator_error").is_none(), "{board}");
        let reviewer = holder(&board).as_str().unwrap().to_owned();
        assert_eq!(board["cards"][1]["holder"], reviewer.as_str());
        let prompt = hub.proj.join(format!(".assistente/prompts/{reviewer}.md"));
        let text = std::fs::read_to_string(&prompt).unwrap();
        assert!(text.starts_with("# Revisão do quadro"), "{text}");
        // A revisão acaba sem tocar nos cards: os mesmos não vão para outra sozinhos.
        t.tmux.kill(&reviewer).await.unwrap();
        let (_, board) = turn().await;
        assert!(holder(&board).is_null(), "{board}");
        assert_eq!(board["cards"][0]["column"], "feito");
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

        // Cota esgotada (F11.14): o motivo vai como texto, com espaços e aspas.
        assert!(client(&["pull"]).await.status.success());
        let out = client(&["bloquear", "T1", "--motivo", "Error: Free usage \"limit\""]).await;
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let card: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(card["column"], "backlog");
        assert_eq!(card["blocked"], "Error: Free usage \"limit\"");

        // QA (F11.15): sem --branch/--nota é uso errado; com, a resposta do hubd volta (aqui,
        // 409: a T1 não está em feito).
        assert_eq!(client(&["validar", "T1"]).await.status.code(), Some(2));
        assert_eq!(client(&["reabrir", "T1"]).await.status.code(), Some(2));
        let out = client(&["reabrir", "T1", "--nota", "falta \"isso\""]).await;
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("HTTP 409"));
        let out = client(&["validar", "T1", "--branch", "main"]).await;
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("HTTP 409"));

        // Revisão (F11.12): juntar sem --commit e resumo sem --texto são uso errado. A main já
        // tem o commit, então juntar sem tarefas não muda nada; o resumo vai com aspas e linhas.
        assert_eq!(client(&["juntar"]).await.status.code(), Some(2));
        assert_eq!(client(&["resumo"]).await.status.code(), Some(2));
        let head = git_sync(&hub.proj, &["rev-parse", "HEAD"]);
        let out = client(&["juntar", "--commit", &head, "--tarefas", "T1"]).await;
        assert_eq!(out.status.code(), Some(1), "a T1 não está em feito");
        assert!(String::from_utf8_lossy(&out.stderr).contains("HTTP 409"));
        let out = client(&["juntar", "--commit", &head]).await;
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(git_sync(&hub.proj, &["rev-parse", "HEAD"]), head);
        let out = client(&["resumo", "--texto", "Entrou: nada.\nVoltou: T1 (\"cota\")."]).await;
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let summary: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(summary["text"], "Entrou: nada.\nVoltou: T1 (\"cota\").");
    }

    #[tokio::test]
    async fn orchestrator_opens_a_dependent_card_only_after_its_dependency_reaches_main() {
        let t = TestTmux::new("kanban-deps");
        let hub = hub(&t, true);
        std::fs::write(
            hub.proj.join(".assistente.toml"),
            "[workers]\nmodels = [\"x/a\"]\n",
        )
        .unwrap();
        git_sync(&hub.proj, &["add", ".assistente.toml"]);
        git_sync(&hub.proj, &["commit", "-q", "-m", "config do projeto"]);
        for body in [
            r#"{"title":"A","kind":"agente"}"#,
            r#"{"title":"B","kind":"agente","priority":"alta","depends_on":["T1"]}"#,
        ] {
            let (status, card) = send(hub.app.clone(), "POST", CARDS, Some(body), None).await;
            assert_eq!(status, StatusCode::CREATED, "{card}");
        }
        let uri = format!("{CARDS}/orquestrador");
        let orchestrate = || {
            let app = hub.app.clone();
            let uri = uri.clone();
            async move {
                let body = r#"{"enabled":true,"max_workers":2}"#;
                send(app, "PUT", &uri, Some(body), None).await
            }
        };
        let mandar_b = || {
            let app = hub.app.clone();
            async move { send(app, "POST", &format!("{CARDS}/T2/mandar"), Some("{}"), None).await }
        };

        // A → B: só A abre, mesmo com B na frente pela prioridade; "Mandar" B também recusa.
        let (status, board) = orchestrate().await;
        assert_eq!(status, StatusCode::OK, "{board}");
        assert_eq!(board["started"], json!(["T1"]), "{board}");
        let (status, body) = mandar_b().await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert!(body["error"].as_str().unwrap().contains("T1"), "{body}");

        // A feito: o commit está só na branch dela, então B continua esperando.
        let holder = board["cards"][0]["holder"].as_str().unwrap().to_owned();
        let worker_token = exec_token(&t, &holder).await;
        let a = worker::worktree_dir(&hub.proj, "T1");
        let commit = crate::sleep::tests::commit(&a, "a.txt", "feito por A");
        let body = format!(r#"{{"id":"T1","commit":"{commit}"}}"#);
        let (status, _) = send(
            hub.agents.clone(),
            "POST",
            "/v1/kanban/done",
            Some(&body),
            Some(&worker_token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, board) = orchestrate().await;
        assert!(board.get("started").is_none(), "{board}");
        assert_eq!(mandar_b().await.0, StatusCode::CONFLICT);
        // Nada em fazendo: a volta abre a revisão (F11.12) com A. Ela acaba sem juntar; o QA junta.
        let reviewer = board["cards"][0]["holder"].as_str().unwrap().to_owned();
        assert_ne!(reviewer, holder, "{board}");
        t.tmux.kill(&reviewer).await.unwrap();

        // O QA junta A na main: B abre, num worktree que já tem o código de A.
        let (status, info) = send(
            hub.app.clone(),
            "POST",
            &format!("{CARDS}/T1/qa"),
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{info}");
        let qa_token = exec_token(&t, info["id"].as_str().unwrap()).await;
        let ((status, done), _) = tokio::join!(
            send(
                hub.agents.clone(),
                "POST",
                "/v1/kanban/validate",
                Some(r#"{"id":"T1","branch":"main"}"#),
                Some(&qa_token),
            ),
            answer_check(hub.app.clone(), Some(0))
        );
        assert_eq!(status, StatusCode::OK, "{done}");
        let (_, board) = orchestrate().await;
        assert_eq!(board["started"], json!(["T2"]), "{board}");
        let b = worker::worktree_dir(&hub.proj, "T2");
        assert_eq!(
            std::fs::read_to_string(b.join("a.txt")).unwrap(),
            "feito por A"
        );
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
