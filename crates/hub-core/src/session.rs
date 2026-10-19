//! Como uma sessão de agente é lançada na VM: tmux → ai-jail → claude | opencode.
//! Spec: docs/specs/02-hubd-e-sessoes.md e docs/specs/05-sandbox.md

use crate::config::{Config, Profile, is_valid_id};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Opencode,
}

impl AgentKind {
    pub fn command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Opencode => "opencode",
        }
    }
}

/// O que a UI pede: "abra o claude no projeto X do perfil Y".
#[derive(Debug, Clone, Deserialize)]
pub struct SessionSpec {
    pub profile: String,
    pub project: String,
    pub agent: AgentKind,
}

/// Tudo que o hubd precisa para criar a sessão; montado sem efeitos colaterais.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub tmux_session: String,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// Comando que roda dentro do tmux.
    pub argv: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchError {
    UnknownProfile(String),
    InvalidProjectName(String),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProfile(id) => write!(f, "perfil desconhecido: {id}"),
            Self::InvalidProjectName(name) => write!(f, "nome de projeto inválido: {name}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Pasta com os dados do perfil (logins do Claude, config do opencode, chave SSH). Fica fora
/// dos dotdirs que o ai-jail monta por padrão: só a sessão do próprio perfil a enxerga.
pub fn profile_data_dir(home: &Path, profile_id: &str) -> PathBuf {
    home.join(".assistente/perfis").join(profile_id)
}

impl LaunchPlan {
    /// `seq` diferencia várias sessões do mesmo agente no mesmo projeto.
    pub fn build(
        config: &Config,
        home: &Path,
        spec: &SessionSpec,
        seq: u32,
    ) -> Result<Self, LaunchError> {
        let profile = config
            .profile(&spec.profile)
            .ok_or_else(|| LaunchError::UnknownProfile(spec.profile.clone()))?;
        if spec.project.is_empty()
            || spec.project.starts_with('.')
            || spec.project.contains(['/', '\\'])
        {
            return Err(LaunchError::InvalidProjectName(spec.project.clone()));
        }
        let data_dir = profile_data_dir(home, &spec.profile);
        let env = profile_env(config, profile, &data_dir);
        Ok(Self {
            tmux_session: tmux_session_name(&spec.profile, &spec.project, spec.agent, seq),
            cwd: profile.root.join(&spec.project),
            argv: jail_argv(config, home, &profile.root, &data_dir, &env, spec.agent),
            env,
        })
    }

    /// `tmux new-session` desanexada; a UI se conecta depois com `tmux attach`.
    pub fn tmux_new_session_argv(&self) -> Vec<String> {
        let mut argv = vec![
            "tmux".to_owned(),
            "new-session".to_owned(),
            "-d".to_owned(),
            "-s".to_owned(),
            self.tmux_session.clone(),
            "-c".to_owned(),
            self.cwd.display().to_string(),
        ];
        for (key, value) in &self.env {
            argv.push("-e".to_owned());
            argv.push(format!("{key}={value}"));
        }
        argv.push("--".to_owned());
        argv.extend(self.argv.iter().cloned());
        argv
    }
}

/// `ai-jail` (2.x) com o mínimo que a sessão precisa. Validado na VM em 2026-10-19:
/// - home privada: só o projeto e os dados do próprio agente são montados;
/// - rede filtrada: o túnel do notebook só aparece com `--forward-port`;
/// - ambiente limpo: cada variável do perfil precisa de `--env NOME` (o valor vem do tmux,
///   então não aparece na linha de comando).
fn jail_argv(
    config: &Config,
    home: &Path,
    profile_root: &Path,
    data_dir: &Path,
    env: &[(String, String)],
    agent: AgentKind,
) -> Vec<String> {
    let mut argv = vec![
        "ai-jail".to_owned(),
        "--rw-map".to_owned(),
        data_dir.display().to_string(),
    ];
    if agent == AgentKind::Claude {
        argv.push("--claude-dir".to_owned());
        argv.push(data_dir.join("claude").display().to_string());
    }
    for port in config.notebook.tunnel.values() {
        argv.push("--forward-port".to_owned());
        argv.push(port.to_string());
    }
    // Hooks do ai-memory: binário (só leitura) e spool local (gravável, para quando o
    // notebook estiver fora).
    argv.push("--map".to_owned());
    argv.push(home.join(".local/lib/ai-memory").display().to_string());
    argv.push("--rw-map".to_owned());
    argv.push(home.join(".local/share/ai-memory").display().to_string());
    // O marcador do workspace mora na raiz do perfil, fora do projeto: sem ele a memória
    // cai no workspace "default" (criado por vm/configurar-agentes.sh).
    argv.push("--map".to_owned());
    argv.push(profile_root.join(".ai-memory.toml").display().to_string());
    for (key, _) in env {
        argv.push("--env".to_owned());
        argv.push(key.clone());
    }
    argv.push("--".to_owned());
    argv.push(agent.command().to_owned());
    argv
}

fn profile_env(config: &Config, profile: &Profile, data_dir: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        // O hubd roda sem locale (systemd) e o ai-jail limpa o ambiente: sem isto o readline e
        // os TUIs leem um acento como atalhos Meta.
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ("ASSISTENTE_PROFILE".to_owned(), profile.label.clone()),
        ("GIT_AUTHOR_NAME".to_owned(), profile.git_name.clone()),
        ("GIT_AUTHOR_EMAIL".to_owned(), profile.git_email.clone()),
        ("GIT_COMMITTER_NAME".to_owned(), profile.git_name.clone()),
        ("GIT_COMMITTER_EMAIL".to_owned(), profile.git_email.clone()),
        (
            "CLAUDE_CONFIG_DIR".to_owned(),
            data_dir.join("claude").display().to_string(),
        ),
        (
            "OPENCODE_CONFIG".to_owned(),
            data_dir
                .join("opencode/opencode.json")
                .display()
                .to_string(),
        ),
    ];
    if let Some(port) = config.notebook.tunnel.get("ai_memory") {
        env.push((
            "AI_MEMORY_SERVER_URL".to_owned(),
            format!("http://127.0.0.1:{port}"),
        ));
    }
    if profile.claude_via_9router
        && let Some(port) = config.notebook.tunnel.get("9router")
    {
        env.push((
            "ANTHROPIC_BASE_URL".to_owned(),
            format!("http://127.0.0.1:{port}"),
        ));
    }
    if let Some(key) = &profile.ssh_key {
        env.push((
            "GIT_SSH_COMMAND".to_owned(),
            format!("ssh -i {} -o IdentitiesOnly=yes", key.display()),
        ));
    }
    env
}

/// tmux não aceita `.` nem `:` no nome; o resto vira `-`.
fn tmux_session_name(profile: &str, project: &str, agent: AgentKind, seq: u32) -> String {
    let project: String = project
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    debug_assert!(is_valid_id(profile));
    format!("{profile}--{project}--{}-{seq}", agent.command())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../config/assistente.exemplo.toml");

    fn setup() -> (Config, PathBuf) {
        let home = PathBuf::from("/home/rafa");
        (Config::parse(EXAMPLE, &home).unwrap(), home)
    }

    fn spec(profile: &str, project: &str, agent: AgentKind) -> SessionSpec {
        SessionSpec {
            profile: profile.into(),
            project: project.into(),
            agent,
        }
    }

    fn env<'a>(plan: &'a LaunchPlan, key: &str) -> Option<&'a str> {
        plan.env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn claude_session_is_jailed_with_only_its_profile_data() {
        let (config, home) = setup();
        let plan = LaunchPlan::build(
            &config,
            &home,
            &spec("pessoal", "game", AgentKind::Claude),
            1,
        )
        .unwrap();
        assert_eq!(plan.tmux_session, "pessoal--game--claude-1");
        assert_eq!(
            plan.cwd,
            PathBuf::from("/home/rafa/Documentos/pessoal/projetos/game")
        );
        let argv = plan.argv.join(" ");
        assert!(argv.starts_with("ai-jail --rw-map /home/rafa/.assistente/perfis/pessoal "));
        assert!(argv.contains("--claude-dir /home/rafa/.assistente/perfis/pessoal/claude"));
        assert!(argv.contains("--forward-port 20128 --forward-port 49374"));
        assert!(argv.contains("--map /home/rafa/.local/lib/ai-memory"));
        assert!(argv.contains("--rw-map /home/rafa/.local/share/ai-memory"));
        assert!(argv.contains("--map /home/rafa/Documentos/pessoal/projetos/.ai-memory.toml"));
        assert!(argv.ends_with(" -- claude"));
        // Toda variável do perfil entra por nome, nunca com o valor na linha de comando.
        for (key, value) in &plan.env {
            assert!(
                argv.contains(&format!("--env {key} ")),
                "{key} não passa pro jail"
            );
            assert!(
                !argv.contains(value.as_str()) || value.starts_with('/'),
                "{key} vazou"
            );
        }
        assert_eq!(
            env(&plan, "CLAUDE_CONFIG_DIR"),
            Some("/home/rafa/.assistente/perfis/pessoal/claude")
        );
        assert_eq!(env(&plan, "GIT_AUTHOR_EMAIL"), Some("voce@exemplo.com"));
        assert_eq!(env(&plan, "LANG"), Some("C.UTF-8"));
        assert_eq!(
            env(&plan, "AI_MEMORY_SERVER_URL"),
            Some("http://127.0.0.1:49374")
        );
        // Exemplo usa a assinatura do Claude direto, sem 9router.
        assert_eq!(env(&plan, "ANTHROPIC_BASE_URL"), None);
    }

    #[test]
    fn profiles_do_not_share_data_dirs() {
        let (config, home) = setup();
        let a = LaunchPlan::build(
            &config,
            &home,
            &spec("pessoal", "x", AgentKind::Opencode),
            1,
        )
        .unwrap();
        let b = LaunchPlan::build(
            &config,
            &home,
            &spec("trabalho", "x", AgentKind::Opencode),
            1,
        )
        .unwrap();
        assert_ne!(a.argv[2], b.argv[2]);
        // opencode não ganha --claude-dir.
        assert!(!a.argv.contains(&"--claude-dir".to_owned()));
        assert_ne!(env(&a, "OPENCODE_CONFIG"), env(&b, "OPENCODE_CONFIG"));
        assert_eq!(env(&b, "GIT_AUTHOR_EMAIL"), Some("voce@empresa.com"));
    }

    #[test]
    fn claude_via_9router_points_to_the_tunnel() {
        let home = PathBuf::from("/home/rafa");
        let text = EXAMPLE.replacen("claude_via_9router = false", "claude_via_9router = true", 1);
        let config = Config::parse(&text, &home).unwrap();
        let plan =
            LaunchPlan::build(&config, &home, &spec("pessoal", "x", AgentKind::Claude), 1).unwrap();
        assert_eq!(
            env(&plan, "ANTHROPIC_BASE_URL"),
            Some("http://127.0.0.1:20128")
        );
    }

    #[test]
    fn rejects_unknown_profile_and_path_tricks() {
        let (config, home) = setup();
        let build = |p: &str, proj: &str| {
            LaunchPlan::build(&config, &home, &spec(p, proj, AgentKind::Claude), 1)
        };
        assert_eq!(
            build("nope", "x").unwrap_err(),
            LaunchError::UnknownProfile("nope".into())
        );
        for bad in ["", "../trabalho", "a/b", ".git"] {
            assert!(
                build("pessoal", bad).is_err(),
                "{bad:?} deveria ser recusado"
            );
        }
    }

    #[test]
    fn tmux_argv_passes_env_and_command() {
        let (config, home) = setup();
        let plan = LaunchPlan::build(
            &config,
            &home,
            &spec("trabalho", "Meu.App", AgentKind::Opencode),
            2,
        )
        .unwrap();
        let argv = plan.tmux_new_session_argv();
        assert_eq!(
            &argv[..5],
            [
                "tmux",
                "new-session",
                "-d",
                "-s",
                "trabalho--meu-app--opencode-2"
            ]
        );
        assert!(argv.contains(&"GIT_AUTHOR_NAME=Seu Nome".to_owned()));
        let dashdash = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(&argv[dashdash + 1..], plan.argv.as_slice());
    }
}
