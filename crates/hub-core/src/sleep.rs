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

/// Formata o snapshot de briefing do ai-memory (`GET
/// /api/v1/workspaces/<w>/projects/<p>/briefing`, o mesmo payload do `memory_briefing`)
/// como texto para o prompt. Tolerante a versões do formato: campo ausente só omite a
/// linha; snapshot vazio vira texto vazio (o prompt fica sem a seção de memória).
pub fn briefing_text(snapshot: &serde_json::Value) -> String {
    let get = |value: &serde_json::Value, path: &[&str]| -> u64 {
        let mut current = value;
        for key in path {
            current = &current[key];
        }
        current.as_u64().unwrap_or(0)
    };
    let mut out = String::new();
    if snapshot.get("counts").is_some() {
        out.push_str(&format!(
            "- Memória: {} páginas, {} sessões. Últimos 7 dias: {} sessões, {} páginas atualizadas.\n",
            get(snapshot, &["counts", "pages_latest"]),
            get(snapshot, &["counts", "sessions"]),
            get(snapshot, &["activity_7d", "sessions"]),
            get(snapshot, &["activity_7d", "pages_updated"]),
        ));
        let pending = get(snapshot, &["pending_handoff_count"]);
        if pending > 0 {
            out.push_str(&format!("- {pending} handoff(s) pendente(s) na memória.\n"));
        }
    }
    for (key, label) in [
        ("rules", "Regras do projeto"),
        ("slots", "Foco atual"),
        ("recent_pages", "Páginas recentes"),
    ] {
        let Some(items) = snapshot.get(key).and_then(|v| v.as_array()) else {
            continue;
        };
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("{label}:\n"));
        for item in items.iter().take(10) {
            let path = item["path"].as_str().unwrap_or("?");
            let title = item["title"].as_str().unwrap_or(path);
            out.push_str(&format!("- {title} ({path})\n"));
        }
    }
    out.trim_end().to_owned()
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
    fn briefing_formats_counts_rules_and_pages() {
        let snapshot = serde_json::json!({
            "counts": { "pages_latest": 138, "pages_all": 162, "sessions": 27, "observations": 4198 },
            "activity_7d": { "days": 7, "sessions": 6, "observations": 921, "pages_updated": 41 },
            "pending_handoff_count": 2,
            "rules": [{ "path": "_rules/postgres.md", "title": "Postgres only" }],
            "slots": [],
            "recent_pages": [
                { "path": "sessions/2026-10-19.md", "title": "Session 2026-10-19" },
                { "path": "decisions/d1.md" }
            ]
        });
        let text = briefing_text(&snapshot);
        assert!(text.contains("138 páginas, 27 sessões"), "{text}");
        assert!(text.contains("7 dias: 6 sessões, 41 páginas"), "{text}");
        assert!(text.contains("2 handoff(s) pendente(s)"), "{text}");
        assert!(
            text.contains("Regras do projeto:\n- Postgres only (_rules/postgres.md)"),
            "{text}"
        );
        assert!(!text.contains("Foco atual"), "slot vazio some: {text}");
        assert!(
            text.contains("- decisions/d1.md (decisions/d1.md)"),
            "sem título usa o path: {text}"
        );
    }

    #[test]
    fn briefing_empty_snapshot_is_empty_text() {
        assert_eq!(briefing_text(&serde_json::json!({})), "");
        assert_eq!(
            briefing_text(&serde_json::json!({"counts": {}})),
            "- Memória: 0 páginas, 0 sessões. Últimos 7 dias: 0 sessões, 0 páginas atualizadas."
        );
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
