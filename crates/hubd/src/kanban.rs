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
    use hub_core::kanban::Column;
    read(dir).map_or(0, |board| {
        board
            .cards
            .iter()
            .filter(|card| matches!(card.column, Column::Backlog | Column::Fazendo))
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
    let mut args = crate::api::hubd_identity_args(dir);
    args.extend(["commit", "-q", "-m", message, "--", FILE].map(str::to_owned));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::api::git_out(dir, &args)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_commits_without_any_git_identity() {
        let dir =
            std::env::temp_dir().join(format!("kanban-sem-identidade-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::api::git_out(&dir, &["init", "-q", "-b", "main"]).unwrap();
        save(&dir, &Board::default(), "kanban: primeira escrita").unwrap();
        let subject = crate::api::git_out(&dir, &["log", "-1", "--format=%s"]).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(subject, "kanban: primeira escrita");
    }
}
