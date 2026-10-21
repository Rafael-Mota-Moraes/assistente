//! Quadro no disco (F11.1): `.assistente/kanban.json` no repo do projeto, com um commit por
//! escrita. Só o arquivo do quadro entra no commit (`git commit -- <arquivo>`): o que você já
//! tinha no index fica do jeito que estava. Lógica das regras em `hub_core::kanban`.

use hub_core::kanban::{Board, FILE};
use std::path::Path;

/// Quadro do projeto; sem arquivo = quadro vazio (o primeiro card cria o arquivo).
pub fn read(dir: &Path) -> Result<Board, String> {
    match std::fs::read_to_string(dir.join(FILE)) {
        Ok(text) => Board::parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Board::default()),
        Err(e) => Err(format!("não consegui ler {FILE}: {e}")),
    }
}

/// Quantas tarefas ainda não estão feitas (backlog + fazendo), para o contador da lista.
pub fn read_pending(dir: &Path) -> usize {
    read(dir).map_or(0, |board| {
        board
            .cards
            .iter()
            .filter(|card| card.column != hub_core::kanban::Column::Feito)
            .count()
    })
}

/// Grava o quadro e comita só ele. Sem mudança = sem commit. Falha de git é erro: a escrita
/// só conta quando está no histórico (o modo dormir depende disso, F6.3).
pub fn save(dir: &Path, board: &Board, message: &str) -> Result<(), String> {
    let path = dir.join(FILE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("pasta do quadro: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, board.to_json()).map_err(|e| format!("gravando o quadro: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("gravando o quadro: {e}"))?;
    crate::api::git_out(dir, &["add", "--", FILE])?;
    if crate::api::git_out(dir, &["status", "--porcelain", "--", FILE])?.is_empty() {
        return Ok(());
    }
    crate::api::git_out(dir, &["commit", "-q", "-m", message, "--", FILE])?;
    Ok(())
}
