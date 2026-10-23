//! Configurações do projeto que afrouxam o ai-jail (F9.2). Ficam em `~/.assistente/projetos.toml`
//! na VM, fora do projeto e de todo mount do jail: o agente escreve no repo, então uma
//! permissão guardada lá ele poderia se dar sozinho. Spec: docs/specs/05-sandbox.md

use crate::config::is_valid_host;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_COMMANDS: usize = 50;
const MAX_COMMAND_LEN: usize = 500;
const MAX_HOSTS: usize = 50;
const MAX_CHECK_LEN: usize = 500;
const DEFAULT_CHECK: &str = "./dev just check";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectSettings {
    /// `--docker`: socket do Docker no jail (equivale a root na VM).
    pub docker: bool,
    /// `--allow-host` extra, para os dois agentes.
    pub allow_hosts: Vec<String>,
    /// `--network`: rede sem filtro. O ai-jail não aceita junto com `--forward-port`.
    pub full_network: bool,
    /// Pedidos do agente (F9.5) que rodam sem perguntar: comparação exata.
    pub allowed_commands: Vec<String>,
    /// Endereço do dev server do projeto (F12.1), visto do PC. `None` = sem preview.
    /// Só `http`/`https` e sem credenciais (valida em [`validate_preview_url`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    /// Check que o `kanban validar` roda antes de juntar (F11.17). Fica aqui, nunca no
    /// `.assistente.toml`: o agente edita esse arquivo, e o comando roda fora do jail.
    pub check: String,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            docker: false,
            allow_hosts: Vec::new(),
            full_network: false,
            allowed_commands: Vec::new(),
            preview_url: None,
            check: DEFAULT_CHECK.to_owned(),
        }
    }
}

/// O arquivo inteiro: perfil → projeto → configurações.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SettingsFile(pub BTreeMap<String, BTreeMap<String, ProjectSettings>>);

impl SettingsFile {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("configurações sempre serializam")
    }

    pub fn get(&self, profile: &str, project: &str) -> ProjectSettings {
        self.0
            .get(profile)
            .and_then(|projects| projects.get(project))
            .cloned()
            .unwrap_or_default()
    }

    /// Voltar ao padrão apaga a entrada: o arquivo só guarda o que foi afrouxado.
    pub fn set(&mut self, profile: &str, project: &str, settings: ProjectSettings) {
        if settings == ProjectSettings::default() {
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
                .insert(project.to_owned(), settings);
        }
    }
}

/// `~/.assistente/projetos.toml` (na VM; fora do `--rw-map` do perfil).
pub fn settings_path(home: &Path) -> PathBuf {
    home.join(".assistente/projetos.toml")
}

/// Lê o arquivo; ausente = vazio. Inválido é erro (melhor que perder permissões em silêncio).
pub fn load_settings(home: &Path) -> Result<SettingsFile, String> {
    let path = settings_path(home);
    match std::fs::read_to_string(&path) {
        Ok(text) => SettingsFile::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SettingsFile::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

impl ProjectSettings {
    /// Hosts com a mesma regra do `opencode_allow_hosts`; comandos de uma linha só.
    pub fn validate(&self) -> Result<(), String> {
        if self.allow_hosts.len() > MAX_HOSTS {
            return Err(format!("no máximo {MAX_HOSTS} hosts liberados"));
        }
        for host in &self.allow_hosts {
            if !is_valid_host(host) {
                return Err(format!(
                    "\"{host}\" não é um nome de host (sem https://, porta ou caminho)"
                ));
            }
        }
        if self.allowed_commands.len() > MAX_COMMANDS {
            return Err(format!(
                "no máximo {MAX_COMMANDS} comandos sempre permitidos"
            ));
        }
        for command in &self.allowed_commands {
            if command.trim().is_empty() {
                return Err("comando sempre permitido vazio".into());
            }
            if command.contains(['\n', '\r']) {
                return Err(format!("comando com quebra de linha: {command:?}"));
            }
            if command.len() > MAX_COMMAND_LEN {
                return Err(format!("comando com mais de {MAX_COMMAND_LEN} caracteres"));
            }
        }
        if let Some(url) = &self.preview_url {
            validate_preview_url(url)?;
        }
        if self.check.trim().is_empty() {
            return Err("comando de verificação vazio".into());
        }
        if self.check.contains(['\n', '\r']) {
            return Err("comando de verificação com quebra de linha".into());
        }
        if self.check.len() > MAX_CHECK_LEN {
            return Err(format!(
                "comando de verificação com mais de {MAX_CHECK_LEN} caracteres"
            ));
        }
        Ok(())
    }
}

/// Valida a URL do preview (F12.1): só `http`/`https`, com host e **sem credenciais**.
///
/// O app abre essa URL numa webview dentro da janela (F12.1) e o arquivo fica na VM,
/// fora do repo: credencial na URL viraria segredo guardado no `projetos.toml`.
/// O mesmo vale para a URL que o app valida antes de abrir a webview.
pub fn validate_preview_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL do preview vazia".into());
    }
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(format!("URL do preview com caractere inválido: {trimmed}"));
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(format!("URL do preview sem http:// ou https://: {trimmed}"));
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Err(format!("URL do preview só http ou https: {trimmed}"));
    }
    // A parte antes do caminho: "host", "host:porta" ou "[::1]:porta".
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(format!("URL do preview sem host: {trimmed}"));
    }
    if authority.contains('@') {
        return Err(format!(
            "URL do preview com credenciais não é permitida: {trimmed}"
        ));
    }
    let host = match authority.strip_prefix('[') {
        Some(ipv6) => ipv6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    if host.is_empty() {
        return Err(format!("URL do preview sem host: {trimmed}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_projects_with_dots_and_drops_defaults() {
        let mut file = SettingsFile::default();
        let loose = ProjectSettings {
            docker: true,
            allow_hosts: vec!["pypi.org".into()],
            full_network: false,
            allowed_commands: vec!["./dev just check".into()],
            preview_url: None,
            check: "./dev just check".into(),
        };
        file.set("pessoal", "Meu.App", loose.clone());
        file.set("trabalho", "erp", ProjectSettings::default());
        let text = file.to_toml();
        let back = SettingsFile::parse(&text).unwrap();
        assert_eq!(back, file);
        assert_eq!(back.get("pessoal", "Meu.App"), loose);
        assert_eq!(back.get("trabalho", "erp"), ProjectSettings::default());
        assert!(!text.contains("trabalho"), "{text}");

        file.set("pessoal", "Meu.App", ProjectSettings::default());
        assert_eq!(file, SettingsFile::default());
    }

    #[test]
    fn unknown_keys_are_an_error() {
        assert!(SettingsFile::parse("[pessoal.game]\nsudo = true\n").is_err());
    }

    #[test]
    fn validate_rejects_urls_and_multiline_commands() {
        let host = ProjectSettings {
            allow_hosts: vec!["https://pypi.org".into()],
            ..Default::default()
        };
        assert!(host.validate().is_err());
        let command = ProjectSettings {
            allowed_commands: vec!["ls\nrm -rf /".into()],
            ..Default::default()
        };
        assert!(command.validate().is_err());
        let empty = ProjectSettings {
            allowed_commands: vec!["  ".into()],
            ..Default::default()
        };
        assert!(empty.validate().is_err());
        let ok = ProjectSettings {
            allow_hosts: vec!["pypi.org".into(), "files.pythonhosted.org".into()],
            allowed_commands: vec!["cargo build".into()],
            ..Default::default()
        };
        assert_eq!(ok.validate(), Ok(()));
    }

    #[test]
    fn validate_check_command() {
        // Vazio
        let empty = ProjectSettings {
            check: "".into(),
            ..Default::default()
        };
        assert!(empty.validate().is_err());
        // Com quebra de linha
        let multiline = ProjectSettings {
            check: "echo a\necho b".into(),
            ..Default::default()
        };
        assert!(multiline.validate().is_err());
        // Muito longo
        let long = ProjectSettings {
            check: "x".repeat(MAX_CHECK_LEN + 1),
            ..Default::default()
        };
        assert!(long.validate().is_err());
        // OK
        let ok = ProjectSettings {
            check: "cargo test".into(),
            ..Default::default()
        };
        assert_eq!(ok.validate(), Ok(()));
        // Default é OK
        let def = ProjectSettings::default();
        assert_eq!(def.validate(), Ok(()));
        // Arquivo de antes da F11.17, sem `check`: o padrão.
        let old = SettingsFile::parse("[pessoal.game]\ndocker = true\n").unwrap();
        assert_eq!(old.get("pessoal", "game").check, "./dev just check");
    }

    #[test]
    fn missing_file_is_empty() {
        let home =
            std::env::temp_dir().join(format!("hub-core-sem-arquivo-{}", std::process::id()));
        assert_eq!(load_settings(&home), Ok(SettingsFile::default()));
    }

    #[test]
    fn preview_url_only_accepts_http_without_credentials() {
        for url in [
            "http://10.77.0.10:5173",
            "https://exemplo.test/app",
            "http://localhost:3000/",
            "http://[::1]:5173",
        ] {
            assert_eq!(validate_preview_url(url), Ok(()), "{url}");
        }
        for url in [
            "",
            "   ",
            "10.77.0.10:5173",
            "ftp://exemplo.test",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "http://",
            "http://usuario:senha@exemplo.test",
        ] {
            assert!(validate_preview_url(url).is_err(), "{url} passou");
        }
    }

    #[test]
    fn preview_url_goes_through_the_settings_validation() {
        let settings = ProjectSettings {
            preview_url: Some("http://10.77.0.10:5173".into()),
            ..Default::default()
        };
        assert_eq!(settings.validate(), Ok(()));
        let ruim = ProjectSettings {
            preview_url: Some("http://usuario:senha@exemplo.test".into()),
            ..Default::default()
        };
        assert!(ruim.validate().is_err());
    }

    #[test]
    fn round_trip_keeps_the_preview_url() {
        let mut file = SettingsFile::default();
        file.set(
            "pessoal",
            "game",
            ProjectSettings {
                preview_url: Some("http://10.77.0.10:5173".into()),
                ..Default::default()
            },
        );
        let text = file.to_toml();
        assert!(text.contains("preview_url"), "{text}");
        let back = SettingsFile::parse(&text).unwrap();
        assert_eq!(
            back.get("pessoal", "game").preview_url.as_deref(),
            Some("http://10.77.0.10:5173")
        );
        // Sem preview, a chave nem aparece no arquivo.
        let mut sem = SettingsFile::default();
        sem.set(
            "pessoal",
            "game",
            ProjectSettings {
                docker: true,
                ..Default::default()
            },
        );
        assert!(!sem.to_toml().contains("preview_url"));
    }
}
