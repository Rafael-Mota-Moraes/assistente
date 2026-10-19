//! Projetos = repositórios git logo abaixo da raiz de um perfil.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    /// Nome da pasta; único dentro do perfil.
    pub name: String,
    pub path: PathBuf,
    /// Tem `.assistente.toml` (config do modo dormir)?
    pub has_project_config: bool,
}

/// Lista os filhos diretos de `root` que são repositórios git (`.git` pasta ou arquivo, que é
/// o caso dos worktrees), em ordem alfabética. Raiz inexistente = nenhum projeto.
pub fn discover_projects(root: &Path) -> std::io::Result<Vec<Project>> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut projects = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir() || !path.join(".git").exists() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        projects.push(Project {
            name: name.to_owned(),
            has_project_config: path.join(".assistente.toml").is_file(),
            path,
        });
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(projects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hub-core-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_git_repos_sorted_and_skips_the_rest() {
        let root = scratch("discover");
        fs::create_dir_all(root.join("zeta/.git")).unwrap();
        fs::create_dir_all(root.join("alfa/.git")).unwrap();
        fs::create_dir_all(root.join("sem-git")).unwrap();
        fs::create_dir_all(root.join(".escondido/.git")).unwrap();
        fs::create_dir_all(root.join("worktree")).unwrap();
        fs::write(root.join("worktree/.git"), "gitdir: /x/.git/worktrees/w\n").unwrap();
        fs::write(root.join("alfa/.assistente.toml"), "[sleep]\n").unwrap();
        fs::write(root.join("arquivo.txt"), "").unwrap();

        let projects = discover_projects(&root).unwrap();
        let names: Vec<_> = projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["alfa", "worktree", "zeta"]);
        assert!(projects[0].has_project_config);
        assert!(!projects[2].has_project_config);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_root_is_empty() {
        let projects = discover_projects(Path::new("/nao/existe/assistente")).unwrap();
        assert!(projects.is_empty());
    }
}
