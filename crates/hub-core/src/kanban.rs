//! Quadro de tarefas de um projeto (F11). Puro: o hubd lê e grava `.assistente/kanban.json`
//! no repo do projeto e comita a cada escrita. Spec: docs/specs/10-kanban-e-trabalhadores.md
//!
//! Dois tipos de card. `manual` (o padrão) só roda quando você manda (`Mandar` na UI). `agente`
//! foi marcado para o agente implementar: o orquestrador e o modo dormir puxam sozinhos.
//!
//! O orquestrador automático (F11.11) é ligado no próprio quadro (`orchestrator`): o hubd abre
//! trabalhadores para as tarefas `agente` até o limite, sem ninguém pedir. Tarefa que voltou do
//! "fazendo" (`failures`) ele não puxa de novo: você decide com "Tentar de novo" ou "Mandar".
//!
//! Cota do provedor esgotada (F11.14) não é falha da tarefa: o trabalhador bloqueia o card
//! (`block`), que volta ao backlog sem contar falha, e o orquestrador fica pausado (`paused`) até
//! você retomar. Enquanto isso, você termina o card com outro harness.
//!
//! QA (F11.15): um card "feito" passa por uma sessão de QA que o segura (`start_qa`). Atende o
//! pedido: a branch é juntada e o card vai para "validado" com `merged` (`validate`). Não atende:
//! volta ao backlog com uma nota do que falta (`reopen`), e o próximo trabalhador continua dali.

use serde::{Deserialize, Serialize};

/// Arquivo do quadro, relativo à raiz do projeto.
pub const FILE: &str = ".assistente/kanban.json";
const MAX_TITLE: usize = 200;
const MAX_DESCRIPTION: usize = 10_000;
/// Trabalhadores simultâneos que o orquestrador automático abre por projeto, no máximo.
pub const MAX_WORKERS: u32 = 4;
const MAX_INSTRUCTIONS: usize = 10_000;
/// Motivo de um bloqueio (a linha de erro do provedor), cortado para caber no card.
const MAX_REASON: usize = 300;
/// Nota do QA (o que falta), somada à descrição do card.
const MAX_NOTE: usize = 2_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Column {
    #[default]
    Backlog,
    Fazendo,
    Feito,
    /// Passou no QA e foi juntado numa branch (F11.15).
    Validado,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Só executa quando você manda.
    #[default]
    Manual,
    /// Marcado para o agente: o orquestrador e o modo dormir puxam.
    Agente,
}

/// Ordem de execução: `Alta` vem primeiro (a ordem das variantes é a de `Ord`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Alta,
    #[default]
    Normal,
    Baixa,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    /// `T1`, `T2`… Nunca reaproveitado, mesmo depois de apagar.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub column: Column,
    /// Sessão do tmux que segura a tarefa ("fazendo"). Sessão morta = tarefa solta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    /// Commit que fechou a tarefa (`feito`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Quantas vezes voltou do "fazendo" sem concluir (trabalhador desistiu ou sessão morreu).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub failures: u32,
    /// Por que o trabalhador parou sem ser culpa da tarefa (cota esgotada, F11.14). Some quando
    /// a tarefa começa de novo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<String>,
    /// Onde o QA juntou a tarefa (`validado`, F11.15).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged: Option<Merged>,
    pub created_at: u64,
}

/// Branch que recebeu a tarefa e o commit dela depois da junção.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merged {
    pub branch: String,
    pub commit: String,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Orquestrador automático do projeto (F11.11): desligado por padrão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Orchestrator {
    pub enabled: bool,
    /// Tarefas em "fazendo" ao mesmo tempo (`1..=MAX_WORKERS`).
    pub max_workers: u32,
    /// Somadas às regras padrão no prompt de todo trabalhador do projeto (automático, Mandar e
    /// reinício). Vazio = só as regras padrão.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub instructions: String,
    /// Pausado por cota esgotada (F11.14): nenhum trabalhador novo abre sozinho até "Retomar".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused: Option<Pause>,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self {
            enabled: false,
            max_workers: 1,
            instructions: String::new(),
            paused: None,
        }
    }
}

/// Por que e quando o orquestrador pausou.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pause {
    pub reason: String,
    pub at: u64,
}

impl Orchestrator {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    /// Número da próxima tarefa (`T<next>`).
    pub next: u32,
    pub cards: Vec<Card>,
    #[serde(skip_serializing_if = "Orchestrator::is_default")]
    pub orchestrator: Orchestrator,
}

/// Dados de um card novo ou de uma edição (todos opcionais na edição).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Draft {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub kind: Option<Kind>,
    pub column: Option<Column>,
    /// `0` = "Tentar de novo": o orquestrador automático volta a puxar a tarefa.
    pub failures: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KanbanError {
    NotFound,
    Invalid(String),
    /// A tarefa não está no backlog (já foi puxada ou concluída).
    NotInBacklog(Column),
    /// Só quem segura a tarefa pode concluí-la ou soltá-la.
    NotHolder,
    /// Concluir ou soltar pede a coluna "fazendo".
    NotInProgress(Column),
    /// QA, validar e reabrir pedem a coluna "feito".
    NotDone(Column),
}

impl std::fmt::Display for KanbanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "tarefa não encontrada"),
            Self::Invalid(why) => write!(f, "{why}"),
            Self::NotInBacklog(column) => write!(f, "a tarefa não está no backlog ({column:?})"),
            Self::NotHolder => write!(f, "a tarefa é de outra sessão"),
            Self::NotInProgress(column) => write!(f, "a tarefa não está em fazendo ({column:?})"),
            Self::NotDone(column) => write!(f, "a tarefa não está em feito ({column:?})"),
        }
    }
}

impl Board {
    /// `text` vazio = quadro novo. JSON inválido é erro (não zera o quadro em silêncio).
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(text).map_err(|e| format!("kanban.json inválido: {e}"))
    }

    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("quadro serializa sempre");
        text.push('\n');
        text
    }

    pub fn get(&self, id: &str) -> Option<&Card> {
        self.cards.iter().find(|card| card.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Result<&mut Card, KanbanError> {
        self.cards
            .iter_mut()
            .find(|card| card.id == id)
            .ok_or(KanbanError::NotFound)
    }

    pub fn create(&mut self, draft: Draft, now: u64) -> Result<&Card, KanbanError> {
        let (title, description) = checked_text(draft.title, draft.description, true)?;
        self.next = self.next.max(1);
        let id = format!("T{}", self.next);
        self.next += 1;
        self.cards.push(Card {
            id,
            title,
            description,
            priority: draft.priority.unwrap_or_default(),
            kind: draft.kind.unwrap_or_default(),
            column: draft.column.unwrap_or_default(),
            holder: None,
            commit: None,
            failures: 0,
            blocked: None,
            merged: None,
            created_at: now,
        });
        Ok(self.cards.last().expect("acabou de ser empurrado"))
    }

    /// Edita campos. Mover de coluna solta a tarefa (sem `holder`), exceto para "fazendo",
    /// que mantém quem a segura.
    pub fn update(&mut self, id: &str, draft: Draft) -> Result<&Card, KanbanError> {
        let card = self.get_mut(id)?;
        if let Some(title) = draft.title {
            card.title = title;
        }
        if let Some(description) = draft.description {
            card.description = description;
        }
        if let Some(priority) = draft.priority {
            card.priority = priority;
        }
        if let Some(kind) = draft.kind {
            card.kind = kind;
        }
        if let Some(failures) = draft.failures {
            card.failures = failures;
        }
        if let Some(column) = draft.column
            && column != card.column
        {
            card.column = column;
            if column != Column::Fazendo {
                card.holder = None;
            }
        }
        let (title, description) = checked_text(
            Some(card.title.clone()),
            Some(card.description.clone()),
            true,
        )?;
        card.title = title;
        card.description = description;
        Ok(card)
    }

    pub fn delete(&mut self, id: &str) -> Result<Card, KanbanError> {
        let index = self
            .cards
            .iter()
            .position(|card| card.id == id)
            .ok_or(KanbanError::NotFound)?;
        Ok(self.cards.remove(index))
    }

    /// Prende uma tarefa do backlog para `holder`. Só o backlog: nunca duas sessões na mesma.
    pub fn claim(&mut self, id: &str, holder: &str) -> Result<&Card, KanbanError> {
        let card = self.get_mut(id)?;
        if card.column != Column::Backlog {
            return Err(KanbanError::NotInBacklog(card.column));
        }
        card.column = Column::Fazendo;
        card.holder = Some(holder.to_owned());
        card.blocked = None;
        Ok(card)
    }

    /// `pull` da API dos agentes: a primeira tarefa `agente` do backlog (prioridade, depois
    /// criação, depois número), já presa a `holder`. `None` = quadro sem trabalho para agente.
    pub fn pull(&mut self, holder: &str) -> Option<&Card> {
        let id = self.first_agent_card(|_| true)?.id.clone();
        self.claim(&id, holder).ok()
    }

    /// Próxima tarefa do orquestrador automático, na ordem do `pull`, sem as que já voltaram do
    /// "fazendo": ele não insiste sozinho no que deu errado. Não prende nada.
    pub fn next_auto(&self) -> Option<&Card> {
        self.first_agent_card(|card| card.failures == 0)
    }

    fn first_agent_card(&self, extra: impl Fn(&Card) -> bool) -> Option<&Card> {
        self.cards
            .iter()
            .filter(|card| card.column == Column::Backlog && card.kind == Kind::Agente)
            .filter(|card| extra(card))
            .min_by_key(|card| (card.priority, card.created_at, number(&card.id)))
    }

    /// Tarefas em "fazendo": o que conta para o limite do orquestrador automático.
    pub fn in_progress(&self) -> usize {
        self.cards
            .iter()
            .filter(|card| card.column == Column::Fazendo)
            .count()
    }

    /// Liga ou desliga o orquestrador automático; `max_workers` vai para `1..=MAX_WORKERS`.
    /// `instructions` `None` mantém as atuais.
    pub fn set_orchestrator(
        &mut self,
        enabled: bool,
        max_workers: u32,
        instructions: Option<String>,
    ) -> Result<&Orchestrator, KanbanError> {
        let instructions = match instructions {
            Some(text) => text.trim().to_owned(),
            None => self.orchestrator.instructions.clone(),
        };
        if instructions.chars().count() > MAX_INSTRUCTIONS {
            return Err(KanbanError::Invalid(format!(
                "instruções com mais de {MAX_INSTRUCTIONS} caracteres"
            )));
        }
        self.orchestrator = Orchestrator {
            enabled,
            max_workers: max_workers.clamp(1, MAX_WORKERS),
            instructions,
            paused: self.orchestrator.paused.take(),
        };
        Ok(&self.orchestrator)
    }

    /// Passa a posse de uma tarefa em andamento (orquestrador → trabalhador).
    pub fn hand_over(&mut self, id: &str, from: &str, to: &str) -> Result<&Card, KanbanError> {
        let card = self.holding(id, from)?;
        card.holder = Some(to.to_owned());
        Ok(card)
    }

    /// Conclui uma tarefa que `holder` segura, com o commit que a fechou (se houver).
    pub fn done(
        &mut self,
        id: &str,
        holder: &str,
        commit: Option<String>,
    ) -> Result<&Card, KanbanError> {
        let card = self.holding(id, holder)?;
        card.column = Column::Feito;
        card.holder = None;
        card.commit = commit;
        Ok(card)
    }

    /// Devolve ao backlog uma tarefa que `holder` segura (sem texto perdido).
    pub fn release(&mut self, id: &str, holder: &str) -> Result<&Card, KanbanError> {
        let card = self.holding(id, holder)?;
        card.column = Column::Backlog;
        card.holder = None;
        card.failures += 1;
        Ok(card)
    }

    /// O provedor recusou por cota (F11.14): a tarefa que `holder` segura volta ao backlog sem
    /// contar falha, com o motivo, e o orquestrador pausa (a primeira pausa fica: é a hora em que
    /// a cota acabou).
    pub fn block(
        &mut self,
        id: &str,
        holder: &str,
        reason: &str,
        now: u64,
    ) -> Result<&Card, KanbanError> {
        let reason: String = reason
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(MAX_REASON)
            .collect();
        let reason = if reason.is_empty() {
            "cota do provedor esgotada".to_owned()
        } else {
            reason
        };
        let card = self.holding(id, holder)?;
        card.column = Column::Backlog;
        card.holder = None;
        card.blocked = Some(reason.clone());
        self.orchestrator
            .paused
            .get_or_insert(Pause { reason, at: now });
        Ok(self.get(id).expect("acabou de ser bloqueado"))
    }

    /// "Retomar": tira a pausa. Os cards bloqueados voltam a ser puxados pelo automático.
    pub fn resume(&mut self) {
        self.orchestrator.paused = None;
    }

    /// QA (F11.15): `holder` passa a segurar uma tarefa "feita" enquanto confere. Uma por vez.
    pub fn start_qa(&mut self, id: &str, holder: &str) -> Result<&Card, KanbanError> {
        let card = self.get_mut(id)?;
        if card.column != Column::Feito {
            return Err(KanbanError::NotDone(card.column));
        }
        if card.holder.is_some() {
            return Err(KanbanError::NotHolder);
        }
        card.holder = Some(holder.to_owned());
        Ok(card)
    }

    /// Passou no QA e foi juntada em `merged`. A sessão continua segurando até acabar: o worktree
    /// só sai depois (`validated_worktrees`).
    pub fn validate(
        &mut self,
        id: &str,
        holder: &str,
        merged: Merged,
    ) -> Result<&Card, KanbanError> {
        let card = self.reviewing(id, holder)?;
        card.column = Column::Validado;
        card.merged = Some(merged);
        Ok(card)
    }

    /// Não passou no QA: volta ao backlog com "Revisão (data): nota" na descrição, sem contar
    /// falha (o automático puxa de novo, e o trabalhador continua da mesma branch).
    pub fn reopen(
        &mut self,
        id: &str,
        holder: &str,
        note: &str,
        date: &str,
    ) -> Result<&Card, KanbanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(KanbanError::Invalid("diga o que falta na nota".to_owned()));
        }
        if note.chars().count() > MAX_NOTE {
            return Err(KanbanError::Invalid(format!(
                "nota com mais de {MAX_NOTE} caracteres"
            )));
        }
        let card = self.reviewing(id, holder)?;
        let description = format!("{}\n\nRevisão ({date}): {note}", card.description)
            .trim()
            .to_owned();
        if description.chars().count() > MAX_DESCRIPTION {
            return Err(KanbanError::Invalid(format!(
                "a descrição passaria de {MAX_DESCRIPTION} caracteres: encurte a nota"
            )));
        }
        card.description = description;
        card.column = Column::Backlog;
        card.holder = None;
        card.commit = None;
        card.failures = 0;
        Ok(card)
    }

    /// Tarefas validadas que ninguém segura mais: o worktree delas pode sair.
    pub fn validated_worktrees(&self) -> impl Iterator<Item = &Card> {
        self.cards
            .iter()
            .filter(|card| card.column == Column::Validado && card.holder.is_none())
    }

    /// Solta as tarefas cuja sessão não existe mais (`alive` diz quais existem). Devolve os ids.
    /// Em "fazendo" a tarefa volta ao backlog com uma falha; fora dele (o QA, F11.15) só perde
    /// quem a segurava.
    pub fn release_dead(&mut self, alive: impl Fn(&str) -> bool) -> Vec<String> {
        let mut released = Vec::new();
        for card in &mut self.cards {
            let Some(holder) = &card.holder else {
                continue;
            };
            if alive(holder) {
                continue;
            }
            card.holder = None;
            if card.column == Column::Fazendo {
                card.column = Column::Backlog;
                card.failures += 1;
            }
            released.push(card.id.clone());
        }
        released
    }

    fn reviewing(&mut self, id: &str, holder: &str) -> Result<&mut Card, KanbanError> {
        let card = self.get_mut(id)?;
        if card.column != Column::Feito {
            return Err(KanbanError::NotDone(card.column));
        }
        if card.holder.as_deref() != Some(holder) {
            return Err(KanbanError::NotHolder);
        }
        Ok(card)
    }

    fn holding(&mut self, id: &str, holder: &str) -> Result<&mut Card, KanbanError> {
        let card = self.get_mut(id)?;
        if card.column != Column::Fazendo {
            return Err(KanbanError::NotInProgress(card.column));
        }
        if card.holder.as_deref() != Some(holder) {
            return Err(KanbanError::NotHolder);
        }
        Ok(card)
    }
}

/// Valida e normaliza título e descrição: título sem espaços nas pontas, obrigatório.
fn checked_text(
    title: Option<String>,
    description: Option<String>,
    require_title: bool,
) -> Result<(String, String), KanbanError> {
    let title = title.unwrap_or_default().trim().to_owned();
    if require_title && title.is_empty() {
        return Err(KanbanError::Invalid("o título é obrigatório".to_owned()));
    }
    if title.chars().count() > MAX_TITLE {
        return Err(KanbanError::Invalid(format!(
            "título com mais de {MAX_TITLE} caracteres"
        )));
    }
    let description = description.unwrap_or_default().trim().to_owned();
    if description.chars().count() > MAX_DESCRIPTION {
        return Err(KanbanError::Invalid(format!(
            "descrição com mais de {MAX_DESCRIPTION} caracteres"
        )));
    }
    Ok((title, description))
}

fn number(id: &str) -> u32 {
    id.trim_start_matches('T').parse().unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str) -> Draft {
        Draft {
            title: Some(title.to_owned()),
            ..Draft::default()
        }
    }

    fn agent_draft(title: &str, priority: Priority) -> Draft {
        Draft {
            kind: Some(Kind::Agente),
            priority: Some(priority),
            ..draft(title)
        }
    }

    #[test]
    fn new_cards_are_manual_normal_and_in_backlog() {
        let mut board = Board::default();
        let card = board.create(draft("primeira"), 10).unwrap().clone();
        assert_eq!(card.id, "T1");
        assert_eq!(card.kind, Kind::Manual);
        assert_eq!(card.priority, Priority::Normal);
        assert_eq!(card.column, Column::Backlog);
        assert_eq!(card.holder, None);
    }

    #[test]
    fn ids_are_never_reused_after_delete() {
        let mut board = Board::default();
        board.create(draft("a"), 1).unwrap();
        board.create(draft("b"), 2).unwrap();
        board.delete("T2").unwrap();
        assert_eq!(board.create(draft("c"), 3).unwrap().id, "T3");
    }

    #[test]
    fn title_is_required_trimmed_and_bounded() {
        let mut board = Board::default();
        assert!(matches!(
            board.create(draft("   "), 1),
            Err(KanbanError::Invalid(_))
        ));
        assert_eq!(board.create(draft("  ok  "), 1).unwrap().title, "ok");
        let long = "x".repeat(MAX_TITLE + 1);
        assert!(matches!(
            board.create(draft(&long), 1),
            Err(KanbanError::Invalid(_))
        ));
    }

    #[test]
    fn pull_takes_only_agent_cards_by_priority_then_age() {
        let mut board = Board::default();
        board.create(draft("manual"), 1).unwrap();
        board
            .create(agent_draft("baixa antiga", Priority::Baixa), 1)
            .unwrap();
        board
            .create(agent_draft("normal", Priority::Normal), 2)
            .unwrap();
        board
            .create(agent_draft("alta nova", Priority::Alta), 9)
            .unwrap();
        assert_eq!(board.pull("s1").unwrap().title, "alta nova");
        assert_eq!(board.pull("s2").unwrap().title, "normal");
        assert_eq!(board.pull("s3").unwrap().title, "baixa antiga");
        // Sobrou só a manual: o agente não pega.
        assert!(board.pull("s4").is_none());
    }

    #[test]
    fn two_sessions_never_get_the_same_card() {
        let mut board = Board::default();
        board.create(agent_draft("a", Priority::Normal), 1).unwrap();
        board.create(agent_draft("b", Priority::Normal), 2).unwrap();
        let first = board.pull("s1").unwrap().id.clone();
        let second = board.pull("s2").unwrap().id.clone();
        assert_ne!(first, second);
        assert!(board.pull("s3").is_none());
    }

    #[test]
    fn only_the_holder_can_finish_or_release() {
        let mut board = Board::default();
        board.create(agent_draft("a", Priority::Normal), 1).unwrap();
        board.pull("s1");
        assert_eq!(board.done("T1", "outra", None), Err(KanbanError::NotHolder));
        assert_eq!(board.release("T1", "outra"), Err(KanbanError::NotHolder));
        let done = board
            .done("T1", "s1", Some("abc123".to_owned()))
            .unwrap()
            .clone();
        assert_eq!(done.column, Column::Feito);
        assert_eq!(done.commit.as_deref(), Some("abc123"));
        assert_eq!(done.holder, None);
    }

    #[test]
    fn done_and_release_need_the_card_in_progress() {
        let mut board = Board::default();
        board.create(draft("a"), 1).unwrap();
        assert_eq!(
            board.done("T1", "s1", None),
            Err(KanbanError::NotInProgress(Column::Backlog))
        );
        assert_eq!(
            board.release("T1", "s1"),
            Err(KanbanError::NotInProgress(Column::Backlog))
        );
    }

    #[test]
    fn release_returns_the_card_to_backlog() {
        let mut board = Board::default();
        board.create(draft("a"), 1).unwrap();
        board.claim("T1", "s1").unwrap();
        let card = board.release("T1", "s1").unwrap().clone();
        assert_eq!(card.column, Column::Backlog);
        assert_eq!(card.holder, None);
        assert_eq!(card.title, "a");
    }

    #[test]
    fn dead_sessions_give_their_cards_back() {
        let mut board = Board::default();
        board.create(draft("a"), 1).unwrap();
        board.create(draft("b"), 2).unwrap();
        board.claim("T1", "morta").unwrap();
        board.claim("T2", "viva").unwrap();
        let released = board.release_dead(|session| session == "viva");
        assert_eq!(released, vec!["T1".to_owned()]);
        assert_eq!(board.get("T1").unwrap().column, Column::Backlog);
        assert_eq!(board.get("T2").unwrap().column, Column::Fazendo);
    }

    #[test]
    fn auto_skips_cards_that_came_back_until_you_retry() {
        let mut board = Board::default();
        board.create(agent_draft("a", Priority::Alta), 1).unwrap();
        board.create(agent_draft("b", Priority::Normal), 2).unwrap();
        board.create(draft("manual"), 3).unwrap();
        assert_eq!(board.next_auto().unwrap().id, "T1");
        board.claim("T1", "s1").unwrap();
        assert_eq!(board.in_progress(), 1);
        board.release("T1", "s1").unwrap();
        board.claim("T2", "s2").unwrap();
        board.release_dead(|_| false);
        assert_eq!(board.get("T1").unwrap().failures, 1);
        assert_eq!(board.get("T2").unwrap().failures, 1);
        // O automático não insiste; o pull explícito (orquestrador Claude, dormir) continua.
        assert!(board.next_auto().is_none());
        assert_eq!(board.in_progress(), 0);
        let retry = Draft {
            failures: Some(0),
            ..Draft::default()
        };
        board.update("T2", retry).unwrap();
        assert_eq!(board.next_auto().unwrap().id, "T2");
    }

    #[test]
    fn quota_blocks_the_card_without_a_failure_and_pauses_until_resume() {
        let mut board = Board::default();
        board.create(agent_draft("a", Priority::Normal), 1).unwrap();
        board.create(agent_draft("b", Priority::Normal), 2).unwrap();
        board.set_orchestrator(true, 2, None).unwrap();
        board.claim("T1", "s1").unwrap();
        board.claim("T2", "s2").unwrap();
        assert_eq!(
            board.block("T1", "outra", "x", 5),
            Err(KanbanError::NotHolder)
        );
        assert_eq!(board.orchestrator.paused, None);
        let card = board
            .block("T1", "s1", "  Error: Free usage limit reached.\n ", 10)
            .unwrap()
            .clone();
        assert_eq!(card.column, Column::Backlog);
        assert_eq!(card.holder, None);
        assert_eq!(card.failures, 0);
        assert_eq!(
            card.blocked.as_deref(),
            Some("Error: Free usage limit reached.")
        );
        // O segundo bloqueio não muda a hora da pausa; mexer no limite não tira a pausa.
        board.block("T2", "s2", "", 20).unwrap();
        assert_eq!(
            board.get("T2").unwrap().blocked.as_deref(),
            Some("cota do provedor esgotada")
        );
        board.set_orchestrator(true, 3, None).unwrap();
        let paused = board.orchestrator.paused.clone().unwrap();
        assert_eq!(paused.at, 10);
        assert_eq!(Board::parse(&board.to_json()).unwrap(), board);
        // Retomar: o automático volta a puxar o bloqueado; começar de novo limpa o motivo.
        board.resume();
        assert_eq!(board.orchestrator.paused, None);
        assert_eq!(board.next_auto().unwrap().id, "T1");
        assert_eq!(board.claim("T1", "s3").unwrap().blocked, None);
    }

    #[test]
    fn qa_validates_or_reopens_with_what_is_missing() {
        let mut board = Board::default();
        board.create(agent_draft("a", Priority::Normal), 1).unwrap();
        board.create(agent_draft("b", Priority::Normal), 2).unwrap();
        assert_eq!(
            board.start_qa("T1", "qa"),
            Err(KanbanError::NotDone(Column::Backlog))
        );
        board.claim("T1", "w").unwrap();
        board.claim("T2", "w").unwrap();
        board.done("T1", "w", Some("abc1".to_owned())).unwrap();
        board.done("T2", "w", Some("abc2".to_owned())).unwrap();
        board.start_qa("T1", "qa1").unwrap();
        board.start_qa("T2", "qa2").unwrap();
        // Uma sessão de QA por vez, e só ela valida.
        assert_eq!(board.start_qa("T1", "outra"), Err(KanbanError::NotHolder));
        let merged = Merged {
            branch: "main".to_owned(),
            commit: "abc9".to_owned(),
        };
        assert_eq!(
            board.validate("T1", "qa2", merged.clone()),
            Err(KanbanError::NotHolder)
        );
        let card = board.validate("T1", "qa1", merged.clone()).unwrap().clone();
        assert_eq!(card.column, Column::Validado);
        assert_eq!(card.merged, Some(merged));
        assert_eq!(card.holder.as_deref(), Some("qa1"));
        // Reabrir pede nota; a nota vai para a descrição e a falha zera.
        assert!(matches!(
            board.reopen("T2", "qa2", "  ", "2026-10-22"),
            Err(KanbanError::Invalid(_))
        ));
        let card = board
            .reopen("T2", "qa2", "falta o botão", "2026-10-22")
            .unwrap()
            .clone();
        assert_eq!(card.column, Column::Backlog);
        assert_eq!(card.holder, None);
        assert_eq!(card.failures, 0);
        assert_eq!(card.commit, None);
        assert_eq!(card.description, "Revisão (2026-10-22): falta o botão");
        assert_eq!(board.next_auto().unwrap().id, "T2");
        // A sessão do QA acabou: o card validado fica, só perde quem segurava.
        assert_eq!(board.validated_worktrees().count(), 0);
        assert_eq!(board.release_dead(|_| false), vec!["T1".to_owned()]);
        let card = board.get("T1").unwrap();
        assert_eq!((card.column, card.failures), (Column::Validado, 0));
        assert_eq!(board.validated_worktrees().count(), 1);
        assert_eq!(Board::parse(&board.to_json()).unwrap(), board);
    }

    #[test]
    fn orchestrator_is_off_by_default_and_bounded() {
        let mut board = Board::default();
        assert!(!board.orchestrator.enabled);
        assert!(!board.to_json().contains("orchestrator"));
        let set = |board: &mut Board, max, text: Option<&str>| {
            board
                .set_orchestrator(true, max, text.map(str::to_owned))
                .cloned()
        };
        assert_eq!(set(&mut board, 0, None).unwrap().max_workers, 1);
        assert_eq!(set(&mut board, 99, None).unwrap().max_workers, MAX_WORKERS);
        // Instruções: aparadas, mantidas quando não vêm, e com limite.
        assert_eq!(
            set(&mut board, 2, Some("  use pnpm  "))
                .unwrap()
                .instructions,
            "use pnpm"
        );
        assert_eq!(set(&mut board, 2, None).unwrap().instructions, "use pnpm");
        let long = "x".repeat(MAX_INSTRUCTIONS + 1);
        assert!(matches!(
            set(&mut board, 2, Some(&long)),
            Err(KanbanError::Invalid(_))
        ));
        assert_eq!(board.orchestrator.instructions, "use pnpm");
        let text = board.to_json();
        assert_eq!(
            Board::parse(&text).unwrap().orchestrator,
            board.orchestrator
        );
    }

    #[test]
    fn hand_over_moves_the_holder_only_for_the_current_one() {
        let mut board = Board::default();
        board.create(agent_draft("a", Priority::Normal), 1).unwrap();
        board.pull("orquestrador");
        assert_eq!(
            board.hand_over("T1", "outro", "trabalhador"),
            Err(KanbanError::NotHolder)
        );
        let card = board
            .hand_over("T1", "orquestrador", "trabalhador")
            .unwrap()
            .clone();
        assert_eq!(card.holder.as_deref(), Some("trabalhador"));
    }

    #[test]
    fn moving_back_to_backlog_drops_the_holder() {
        let mut board = Board::default();
        board.create(draft("a"), 1).unwrap();
        board.claim("T1", "s1").unwrap();
        let card = board
            .update(
                "T1",
                Draft {
                    column: Some(Column::Backlog),
                    ..Draft::default()
                },
            )
            .unwrap()
            .clone();
        assert_eq!(card.column, Column::Backlog);
        assert_eq!(card.holder, None);
    }

    #[test]
    fn round_trip_keeps_the_board_and_empty_text_is_a_new_board() {
        let mut board = Board::default();
        board.create(agent_draft("a", Priority::Alta), 1).unwrap();
        board.pull("s1");
        let text = board.to_json();
        assert_eq!(Board::parse(&text).unwrap(), board);
        assert_eq!(Board::parse("  \n").unwrap(), Board::default());
        assert!(Board::parse("{quebrado").is_err());
    }
}
