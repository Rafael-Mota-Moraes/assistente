//! `~/.config/assistente/assistente.toml`. Exemplo comentado: `config/assistente.exemplo.toml`.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub hub: HubConfig,
    pub vm: VmConfig,
    pub notebook: NotebookConfig,
    pub memory: MemoryConfig,
    /// Ordenado pelo id (`BTreeMap`) para a UI listar sempre na mesma ordem.
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HubConfig {
    pub listen: String,
    pub token_file: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmConfig {
    pub name: String,
    pub image: String,
    pub cpus: u32,
    pub memory: String,
    pub disk: String,
    pub ip: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotebookConfig {
    pub ssh_host: String,
    /// IP ou nome (LAN/Tailscale) e usuário; viram o alias `ssh_host` no ~/.ssh/config da VM.
    pub address: String,
    pub user: String,
    pub workdir: PathBuf,
    /// Nome do serviço → porta no notebook, encaminhada para a mesma porta em 127.0.0.1 da VM.
    pub tunnel: BTreeMap<String, u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryConfig {
    pub mirror_dir: PathBuf,
    pub mirror_every_minutes: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub label: String,
    pub color: String,
    pub root: PathBuf,
    pub git_name: String,
    pub git_email: String,
    #[serde(default)]
    pub ssh_key: Option<PathBuf>,
    pub memory_workspace: String,
    #[serde(default)]
    pub opencode_model: Option<String>,
    #[serde(default)]
    pub opencode_small_model: Option<String>,
    /// Hosts que o proxy do ai-jail libera para o opencode (provedores fora do 9router).
    #[serde(default)]
    pub opencode_allow_hosts: Vec<String>,
    #[serde(default)]
    pub claude_via_9router: bool,
}

#[derive(Debug)]
pub enum ConfigError {
    Read(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path, e) => write!(f, "não consegui ler {}: {e}", path.display()),
            Self::Parse(path, e) => write!(f, "config inválida em {}: {e}", path.display()),
            Self::Invalid(msg) => write!(f, "config inválida: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Lê e valida a config, expandindo `~` com `home` (passado de fora para os testes).
    pub fn load(path: &Path, home: &Path) -> Result<Self, ConfigError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| ConfigError::Read(path.to_owned(), e))?;
        Self::parse(&text, home).map_err(|e| match e {
            ConfigError::Parse(_, inner) => ConfigError::Parse(path.to_owned(), inner),
            other => other,
        })
    }

    pub fn parse(text: &str, home: &Path) -> Result<Self, ConfigError> {
        let mut config: Self =
            toml::from_str(text).map_err(|e| ConfigError::Parse(PathBuf::new(), e))?;
        config.expand_paths(home);
        config.validate()?;
        Ok(config)
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.get(id)
    }

    fn expand_paths(&mut self, home: &Path) {
        self.hub.token_file = expand_home(&self.hub.token_file, home);
        self.memory.mirror_dir = expand_home(&self.memory.mirror_dir, home);
        for profile in self.profiles.values_mut() {
            profile.root = expand_home(&profile.root, home);
            profile.ssh_key = profile.ssh_key.as_deref().map(|p| expand_home(p, home));
        }
        // `notebook.workdir` é um caminho NO NOTEBOOK: o `~` fica para o shell remoto expandir.
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.profiles.is_empty() {
            return Err(ConfigError::Invalid(
                "defina ao menos um [profiles.<id>]".into(),
            ));
        }
        for (id, profile) in &self.profiles {
            if !is_valid_id(id) {
                return Err(ConfigError::Invalid(format!(
                    "id de perfil \"{id}\": use só letras minúsculas, números e '-'"
                )));
            }
            for host in &profile.opencode_allow_hosts {
                if !is_valid_host(host) {
                    return Err(ConfigError::Invalid(format!(
                        "perfil {id}: opencode_allow_hosts \"{host}\" não é um nome de host \
                         (sem https://, porta ou caminho)"
                    )));
                }
            }
        }
        // Raízes aninhadas misturariam projetos (e credenciais) de perfis diferentes.
        let roots: Vec<(&String, &PathBuf)> =
            self.profiles.iter().map(|(id, p)| (id, &p.root)).collect();
        for (i, (a, root_a)) in roots.iter().enumerate() {
            for (b, root_b) in roots.iter().skip(i + 1) {
                if root_a.starts_with(root_b) || root_b.starts_with(root_a) {
                    return Err(ConfigError::Invalid(format!(
                        "as raízes dos perfis {a} e {b} se sobrepõem"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Ids viram nome de sessão do tmux e pasta em `~/.assistente/perfis/<id>`.
pub(crate) fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Hosts viram `--allow-host` do ai-jail, que libera o host e os subdomínios.
fn is_valid_host(host: &str) -> bool {
    !host.is_empty()
        && !host.starts_with(['.', '-'])
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// `~` e `~/x` viram caminhos dentro de `home`; o resto fica igual.
pub fn expand_home(path: &Path, home: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../config/assistente.exemplo.toml");

    fn home() -> PathBuf {
        PathBuf::from("/home/rafa")
    }

    #[test]
    fn example_config_parses() {
        let config = Config::parse(EXAMPLE, &home()).expect("exemplo precisa ser válido");
        assert_eq!(
            config.profiles.keys().collect::<Vec<_>>(),
            ["pessoal", "trabalho"]
        );
        assert_eq!(config.notebook.tunnel["9router"], 20128);
        assert_eq!(config.notebook.tunnel["ai_memory"], 49374);
        assert_eq!(
            config.profile("pessoal").unwrap().root,
            PathBuf::from("/home/rafa/Documentos/pessoal/projetos")
        );
        assert_eq!(
            config.hub.token_file,
            PathBuf::from("/home/rafa/.config/assistente/hub.token")
        );
    }

    #[test]
    fn overlapping_roots_are_rejected() {
        let text = EXAMPLE.replace(
            "~/Documentos/trabalho",
            "~/Documentos/pessoal/projetos/firma",
        );
        let err = Config::parse(&text, &home()).unwrap_err();
        assert!(err.to_string().contains("se sobrepõem"), "{err}");
    }

    #[test]
    fn profile_id_must_be_safe() {
        let text = EXAMPLE.replace("[profiles.trabalho]", "[profiles.\"Trab Alho\"]");
        assert!(Config::parse(&text, &home()).is_err());
    }

    #[test]
    fn opencode_allow_hosts_must_be_bare_hosts() {
        let with = |hosts: &str| {
            EXAMPLE.replacen(
                "# opencode_allow_hosts = [\"api.meu-provedor.com\"]",
                &format!("opencode_allow_hosts = {hosts}"),
                1,
            )
        };
        let config = Config::parse(&with(r#"["api.exemplo.com"]"#), &home()).unwrap();
        assert_eq!(
            config.profile("pessoal").unwrap().opencode_allow_hosts,
            ["api.exemplo.com"]
        );
        for bad in [
            r#"["https://api.exemplo.com"]"#,
            r#"["api.exemplo.com/v1"]"#,
            r#"[""]"#,
        ] {
            let err = Config::parse(&with(bad), &home()).unwrap_err();
            assert!(err.to_string().contains("opencode_allow_hosts"), "{err}");
        }
    }

    #[test]
    fn expand_home_only_touches_tilde() {
        assert_eq!(expand_home(Path::new("~/a/b"), &home()), home().join("a/b"));
        assert_eq!(expand_home(Path::new("~"), &home()), home());
        assert_eq!(
            expand_home(Path::new("/etc/x"), &home()),
            PathBuf::from("/etc/x")
        );
        assert_eq!(
            expand_home(Path::new("~x/y"), &home()),
            PathBuf::from("~x/y")
        );
    }
}
