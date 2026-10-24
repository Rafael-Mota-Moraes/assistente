//! Lista curta de comandos exatos que rodam **no PC** sem clique (F9.11). Fica no host, em
//! `~/.config/assistente/pc-permitidos.toml`: a VM só monta essa pasta como leitura, então nem
//! o agente nem o hubd conseguem se dar permissão. Quem lê e decide é o app.
//! Spec: docs/specs/05-sandbox.md

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// "No máximo uma lista curta": o PC é a fronteira mais forte (R3).
pub const MAX_PC_COMMANDS: usize = 10;
const MAX_COMMAND_LEN: usize = 200;

/// O arquivo inteiro: perfil → projeto → comandos (texto exato).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PcAllowList(pub BTreeMap<String, BTreeMap<String, Vec<String>>>);

impl PcAllowList {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("lista do PC sempre serializa")
    }

    pub fn get(&self, profile: &str, project: &str) -> Vec<String> {
        self.0
            .get(profile)
            .and_then(|projects| projects.get(project))
            .cloned()
            .unwrap_or_default()
    }

    /// Lista vazia apaga a entrada: o arquivo só guarda o que foi liberado.
    pub fn set(&mut self, profile: &str, project: &str, commands: Vec<String>) {
        if commands.is_empty() {
            if let Some(projects) = self.0.get_mut(profile) {
                projects.remove(project);
                if projects.is_empty() {
                    self.0.remove(profile);
                }
            }
        } else {
            self.0
                .entry(profile.to_owned())
                .or_default()
                .insert(project.to_owned(), commands);
        }
    }

    /// Comparação exata, sem aparar espaços nem casar prefixo: `git push` não libera
    /// `git push --force` nem `git push; rm -rf ~`.
    pub fn allows(&self, profile: &str, project: &str, command: &str) -> bool {
        self.0
            .get(profile)
            .and_then(|projects| projects.get(project))
            .is_some_and(|commands| commands.iter().any(|c| c == command))
    }
}

/// Valida a lista de um projeto antes de gravar.
pub fn validate_pc_commands(commands: &[String]) -> Result<(), String> {
    if commands.len() > MAX_PC_COMMANDS {
        return Err(format!(
            "no máximo {MAX_PC_COMMANDS} comandos sem clique no PC"
        ));
    }
    for command in commands {
        if command.trim().is_empty() {
            return Err("comando vazio".into());
        }
        if command.trim() != command {
            return Err(format!("comando com espaço nas pontas: {command:?}"));
        }
        if command.contains(['\n', '\r']) {
            return Err(format!("comando com quebra de linha: {command:?}"));
        }
        if command.len() > MAX_COMMAND_LEN {
            return Err(format!("comando com mais de {MAX_COMMAND_LEN} caracteres"));
        }
    }
    Ok(())
}

/// `~/.config/assistente/pc-permitidos.toml` (no host).
pub fn pc_allow_path(home: &Path) -> PathBuf {
    home.join(".config/assistente/pc-permitidos.toml")
}

/// Lê o arquivo; ausente = vazio. Inválido é erro (melhor nada rodar sozinho do que adivinhar).
pub fn load_pc_allow(home: &Path) -> Result<PcAllowList, String> {
    let path = pc_allow_path(home);
    match std::fs::read_to_string(&path) {
        Ok(text) => PcAllowList::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PcAllowList::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Grava a lista de um projeto (lê, muda, escreve via tmp + rename, modo 0600).
pub fn save_pc_commands(
    home: &Path,
    profile: &str,
    project: &str,
    commands: Vec<String>,
) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    validate_pc_commands(&commands)?;
    let mut list = load_pc_allow(home)?;
    list.set(profile, project, commands);
    let path = pc_allow_path(home);
    let dir = path.parent().expect("caminho tem pasta");
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = path.with_extension("toml.tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    file.write_all(list.to_toml().as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pc-allow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn so_o_texto_exato_do_mesmo_projeto_passa() {
        let mut list = PcAllowList::default();
        list.set("pessoal", "game", vec!["git push".into()]);
        assert!(list.allows("pessoal", "game", "git push"));
        assert!(!list.allows("pessoal", "game", "git push --force"));
        assert!(!list.allows("pessoal", "game", "git push; rm -rf ~"));
        assert!(!list.allows("pessoal", "game", " git push"));
        assert!(!list.allows("pessoal", "outro", "git push"));
        assert!(!list.allows("trabalho", "game", "git push"));
    }

    #[test]
    fn lista_e_curta_e_de_uma_linha() {
        assert!(validate_pc_commands(&["git push".into()]).is_ok());
        let long: Vec<String> = (0..=MAX_PC_COMMANDS).map(|i| format!("cmd {i}")).collect();
        assert!(validate_pc_commands(&long).is_err());
        assert!(validate_pc_commands(&["git push\nrm -rf ~".into()]).is_err());
        assert!(validate_pc_commands(&["  ".into()]).is_err());
        assert!(validate_pc_commands(&["git push ".into()]).is_err());
    }

    #[test]
    fn grava_le_e_lista_vazia_apaga() {
        let home = temp_home("roundtrip");
        assert_eq!(load_pc_allow(&home).unwrap(), PcAllowList::default());
        save_pc_commands(&home, "pessoal", "game", vec!["git push".into()]).unwrap();
        let list = load_pc_allow(&home).unwrap();
        assert_eq!(list.get("pessoal", "game"), vec!["git push".to_string()]);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(pc_allow_path(&home))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        save_pc_commands(&home, "pessoal", "game", vec![]).unwrap();
        assert_eq!(load_pc_allow(&home).unwrap(), PcAllowList::default());
        assert!(save_pc_commands(&home, "pessoal", "game", vec!["a\nb".into()]).is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn arquivo_invalido_e_erro() {
        let home = temp_home("invalido");
        let path = pc_allow_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "isso não é toml =").unwrap();
        assert!(load_pc_allow(&home).is_err());
        let _ = std::fs::remove_dir_all(&home);
    }
}
