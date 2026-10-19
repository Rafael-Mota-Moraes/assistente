//! Modo "hora de dormir": config por projeto (`.assistente.toml`) e o prompt do agente noturno.
//! O loop em si roda no notebook (`notebook/dorme`); o hubd gera o prompt e o envia junto.
//! Spec: docs/specs/08-modo-dormir.md

use serde::Deserialize;
use std::path::Path;

const PROMPT_TEMPLATE: &str = include_str!("sleep_prompt.md");

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub sleep: SleepConfig,
    #[serde(default)]
    pub jail: JailConfig,
}

/// Opções do ai-jail por projeto (spec 05). Hoje só Docker para `./dev`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JailConfig {
    pub docker: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SleepConfig {
    pub backlog: String,
    pub section: String,
    pub check: String,
    pub human_only: Vec<String>,
    pub max_minutes: u32,
    pub attempts: u32,
    pub model: String,
    pub extra_instructions: String,
}

impl Default for SleepConfig {
    fn default() -> Self {
        Self {
            backlog: "docs/FUNCIONALIDADES.md".into(),
            section: String::new(),
            check: "./dev just check".into(),
            human_only: Vec::new(),
            max_minutes: 240,
            attempts: 3,
            model: String::new(),
            extra_instructions: String::new(),
        }
    }
}

impl ProjectConfig {
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

/// Lê `.assistente.toml` na raiz do repo; ausente ou inválido → padrões (Docker desligado).
pub fn jail_config_at(project_root: &Path) -> JailConfig {
    let path = project_root.join(".assistente.toml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return JailConfig::default();
    };
    ProjectConfig::parse(&text)
        .map(|c| c.jail)
        .unwrap_or_default()
}

/// Prompt de uma iteração. `base` é a branch em que o notebook trabalha (`dorme/<data>`);
/// `briefing` é o resumo do ai-memory exportado na hora de dormir (pode ser vazio).
pub fn render_prompt(config: &SleepConfig, base: &str, briefing: &str) -> String {
    let section = if config.section.trim().is_empty() {
        String::new()
    } else {
        format!(", seção \"{}\"", config.section.trim())
    };
    let human_only = config
        .human_only
        .iter()
        .map(|item| format!("- NUNCA escolha item que contenha \"{item}\": só humano fecha.\n"))
        .collect::<String>();
    let extra = match config.extra_instructions.trim() {
        "" => String::new(),
        text => format!("\nREGRAS DO PROJETO:\n{text}\n"),
    };
    let briefing = match briefing.trim() {
        "" => String::new(),
        text => format!("\nCONTEXTO DA MEMÓRIA (ai-memory, exportado ao iniciar):\n{text}\n"),
    };
    PROMPT_TEMPLATE
        .replace("@BACKLOG@", &config.backlog)
        .replace("@SECTION@", &section)
        .replace("@HUMAN_ONLY@", &human_only)
        .replace("@CHECK@", &config.check)
        .replace("@BASE@", base)
        .replace("@EXTRA@", &extra)
        .replace("@BRIEFING@", &briefing)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../config/projeto.exemplo.toml");

    #[test]
    fn example_project_config_parses() {
        let config = ProjectConfig::parse(EXAMPLE).unwrap();
        assert_eq!(config.sleep.backlog, "docs/FUNCIONALIDADES.md");
        assert_eq!(config.sleep.check, "./dev just check");
        assert_eq!(config.sleep.attempts, 3);
    }

    #[test]
    fn empty_file_uses_defaults() {
        let config = ProjectConfig::parse("").unwrap();
        assert_eq!(config.sleep.max_minutes, 240);
        assert!(!config.jail.docker);
    }

    #[test]
    fn jail_docker_parses_from_example() {
        let text = include_str!("../../../config/projeto.exemplo.toml")
            .replace("docker = false", "docker = true");
        let config = ProjectConfig::parse(&text).unwrap();
        assert!(config.jail.docker);
    }

    #[test]
    fn prompt_fills_every_placeholder() {
        let mut config = SleepConfig {
            section: "Mapa político".into(),
            human_only: vec!["playtest".into()],
            extra_instructions: "Código em inglês.".into(),
            ..SleepConfig::default()
        };
        let prompt = render_prompt(&config, "dorme/2026-10-19", "Último trabalho: MP2.");
        assert!(!prompt.contains('@'), "placeholder sobrando:\n{prompt}");
        assert!(prompt.contains("docs/FUNCIONALIDADES.md, seção \"Mapa político\""));
        assert!(prompt.contains("a partir de dorme/2026-10-19"));
        assert!(prompt.contains("contenha \"playtest\""));
        assert!(prompt.contains("REGRAS DO PROJETO:\nCódigo em inglês."));
        assert!(prompt.contains("Último trabalho: MP2."));

        config.extra_instructions.clear();
        let bare = render_prompt(&config, "main", "");
        assert!(!bare.contains("REGRAS DO PROJETO"));
        assert!(!bare.contains("CONTEXTO DA MEMÓRIA"));
    }
}
