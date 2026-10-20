//! Como uma sessão de agente é lançada na VM: tmux → ai-jail → claude | opencode. A aba
//! Terminal (`AgentKind::Shell`) é a exceção: bash direto no tmux, fora do jail (F9.1).
//! Spec: docs/specs/02-hubd-e-sessoes.md e docs/specs/05-sandbox.md

use crate::config::{Config, Profile, is_valid_id};
use crate::project_settings::ProjectSettings;
use crate::sleep::jail_config_at;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Opencode,
    /// Aba Terminal: bash na VM, fora do jail, com o env do perfil (F9.1).
    Shell,
}

impl AgentKind {
    /// Nome no tmux (`…--claude-1`) e na opção `@assistente_agent`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::Shell => "shell",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::Claude, Self::Opencode, Self::Shell]
            .into_iter()
            .find(|a| a.id() == id)
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

/// O que o hubd lê de fora (arquivos, segredos) e entrega pronto: o `LaunchPlan` fica puro.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    /// Configurações do projeto em `~/.assistente/projetos.toml` (F9.2).
    pub settings: ProjectSettings,
}

/// O jail efetivo: configurações do projeto + `[jail]` do `.assistente.toml` (compatibilidade).
struct JailOptions<'a> {
    docker: bool,
    full_network: bool,
    allow_hosts: &'a [String],
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
        Self::build_with(config, home, spec, seq, &LaunchOptions::default())
    }

    pub fn build_with(
        config: &Config,
        home: &Path,
        spec: &SessionSpec,
        seq: u32,
        options: &LaunchOptions,
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
        let cwd = profile.root.join(&spec.project);
        let argv = if spec.agent == AgentKind::Shell {
            // Login: pega o PATH do usuário (~/.local/bin) como num terminal comum.
            vec!["bash".to_owned(), "-l".to_owned()]
        } else {
            let settings = &options.settings;
            let jail = JailOptions {
                docker: settings.docker || jail_config_at(&cwd).docker,
                full_network: settings.full_network,
                allow_hosts: &settings.allow_hosts,
            };
            jail_argv(config, home, profile, &data_dir, &env, spec.agent, &jail)
        };
        Ok(Self {
            tmux_session: tmux_session_name(&spec.profile, &spec.project, spec.agent, seq),
            cwd,
            argv,
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
    profile: &Profile,
    data_dir: &Path,
    env: &[(String, String)],
    agent: AgentKind,
    jail: &JailOptions,
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
    if jail.full_network {
        // Rede sem filtro (F9.2): o ai-jail recusa `--forward-port` junto, e o filtro de
        // hosts deixa de existir.
        argv.push("--network".to_owned());
    } else {
        for (name, port) in &config.notebook.tunnel {
            if name == "ai_memory" {
                continue;
            }
            argv.push("--forward-port".to_owned());
            argv.push(port.to_string());
        }
        // O ai-memory é local na VM (F5.0), mas o sandbox tem loopback próprio: sem o relay os
        // hooks/MCP do agente não alcançam 127.0.0.1:porta e a captura cai no spool para sempre.
        argv.push("--forward-port".to_owned());
        argv.push(config.memory.port.to_string());
        // Provedores do opencode fora do 9router (copiados do PC): o proxy do jail só libera os
        // hosts do preset. Soma à lista do preset; o Claude não ganha esses hosts.
        if agent == AgentKind::Opencode {
            for host in &profile.opencode_allow_hosts {
                argv.push("--allow-host".to_owned());
                argv.push(host.clone());
            }
        }
        // Hosts liberados pela engrenagem do projeto (F9.2): valem para os dois agentes.
        for host in jail.allow_hosts {
            argv.push("--allow-host".to_owned());
            argv.push(host.clone());
        }
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
    argv.push(profile.root.join(".ai-memory.toml").display().to_string());
    // Sem isto o proxy de terminal do ai-jail descarta o OSC 52, e copiar com o mouse no
    // claude/opencode (que prendem o mouse e copiam sozinhos) não chega ao app. Do outro lado
    // está o tmux, que só repassa ao app o OSC 52 de escrita (set-clipboard on).
    argv.push("--terminal-passthrough".to_owned());
    if jail.docker {
        argv.push("--docker".to_owned());
    }
    for (key, _) in env {
        argv.push("--env".to_owned());
        argv.push(key.clone());
    }
    argv.push("--".to_owned());
    argv.push(agent.id().to_owned());
    // O opencode 2 roda como cliente de um servidor em segundo plano compartilhado, que não
    // sobrevive ao jail: com `--standalone` cada sessão sobe o seu.
    if agent == AgentKind::Opencode {
        argv.push("--standalone".to_owned());
    }
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
    env.push((
        "AI_MEMORY_SERVER_URL".to_owned(),
        format!("http://127.0.0.1:{}", config.memory.port),
    ));
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
    format!("{profile}--{project}--{}-{seq}", agent.id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        assert!(argv.contains("--forward-port 20128"));
        // ai-memory é local na VM (F5.0): o túnel ignora a entrada legacy, mas o relay para o
        // sandbox continua necessário (loopback do jail é isolado) — exatamente uma vez.
        assert_eq!(argv.matches("--forward-port 49374").count(), 1);
        assert!(argv.contains("--map /home/rafa/.local/lib/ai-memory"));
        assert!(argv.contains("--rw-map /home/rafa/.local/share/ai-memory"));
        assert!(argv.contains("--map /home/rafa/Documentos/pessoal/projetos/.ai-memory.toml"));
        assert!(argv.contains(" --terminal-passthrough "));
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
        assert_eq!(
            a.argv[2],
            "/home/rafa/.assistente/perfis/pessoal".to_owned()
        );
        assert_eq!(
            b.argv[2],
            "/home/rafa/.assistente/perfis/trabalho".to_owned()
        );
        let a_join = a.argv.join(" ");
        assert!(!a_join.contains("/perfis/trabalho"));
        assert!(!b.argv.join(" ").contains("/perfis/pessoal "));
        // opencode não ganha --claude-dir.
        assert!(!a.argv.contains(&"--claude-dir".to_owned()));
        assert_ne!(env(&a, "OPENCODE_CONFIG"), env(&b, "OPENCODE_CONFIG"));
        assert_eq!(env(&b, "GIT_AUTHOR_EMAIL"), Some("voce@empresa.com"));
    }

    #[test]
    fn only_opencode_gets_the_profile_allowed_hosts() {
        let home = PathBuf::from("/home/rafa");
        let text = EXAMPLE.replacen(
            "# opencode_allow_hosts = [\"api.meu-provedor.com\"]",
            "opencode_allow_hosts = [\"api.exemplo.com\", \"outro.exemplo.com\"]",
            1,
        );
        let config = Config::parse(&text, &home).unwrap();
        let build = |agent| LaunchPlan::build(&config, &home, &spec("pessoal", "x", agent), 1);
        let opencode = build(AgentKind::Opencode).unwrap().argv.join(" ");
        assert!(
            opencode.contains("--allow-host api.exemplo.com --allow-host outro.exemplo.com"),
            "{opencode}"
        );
        assert!(
            opencode.ends_with(" -- opencode --standalone"),
            "{opencode}"
        );
        let claude = build(AgentKind::Claude).unwrap().argv;
        assert!(!claude.contains(&"--allow-host".to_owned()));
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
    fn shell_session_runs_bash_outside_the_jail() {
        let (config, home) = setup();
        let plan = LaunchPlan::build(
            &config,
            &home,
            &spec("pessoal", "game", AgentKind::Shell),
            1,
        )
        .unwrap();
        assert_eq!(plan.tmux_session, "pessoal--game--shell-1");
        assert_eq!(plan.argv, ["bash", "-l"]);
        assert_eq!(
            plan.cwd,
            PathBuf::from("/home/rafa/Documentos/pessoal/projetos/game")
        );
        // Mesmo env das sessões de agente: identidade git, config do perfil.
        assert_eq!(env(&plan, "GIT_AUTHOR_EMAIL"), Some("voce@exemplo.com"));
        assert_eq!(env(&plan, "LANG"), Some("C.UTF-8"));
        assert_eq!(AgentKind::from_id("shell"), Some(AgentKind::Shell));
        assert_eq!(AgentKind::from_id("bash"), None);
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

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hub-core-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn docker_flag_when_project_config_enables_it() {
        let projects = scratch("jail-docker");
        let home = PathBuf::from("/home/rafa");
        let text = EXAMPLE.replacen(
            "root = \"~/Documentos/pessoal/projetos\"",
            &format!(r#"root = "{}""#, projects.display()),
            1,
        );
        let config = Config::parse(&text, &home).unwrap();
        let game = projects.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join(".assistente.toml"), "[jail]\ndocker = true\n").unwrap();
        let plan = LaunchPlan::build(
            &config,
            &home,
            &spec("pessoal", "game", AgentKind::Opencode),
            1,
        )
        .unwrap();
        assert!(plan.argv.contains(&"--docker".to_owned()));
        fs::remove_dir_all(projects).unwrap();
    }

    fn with_settings(settings: ProjectSettings) -> LaunchOptions {
        LaunchOptions { settings }
    }

    #[test]
    fn project_settings_loosen_the_jail() {
        let (config, home) = setup();
        let options = with_settings(ProjectSettings {
            docker: true,
            allow_hosts: vec!["pypi.org".into()],
            ..Default::default()
        });
        for agent in [AgentKind::Claude, AgentKind::Opencode] {
            let plan =
                LaunchPlan::build_with(&config, &home, &spec("pessoal", "x", agent), 1, &options)
                    .unwrap();
            let argv = plan.argv.join(" ");
            assert!(argv.contains(" --docker "), "{argv}");
            assert!(argv.contains("--allow-host pypi.org"), "{argv}");
            assert!(!argv.contains("--network"), "{argv}");
        }
        // A aba Terminal já está fora do jail: as configurações não mudam nada nela.
        let shell = LaunchPlan::build_with(
            &config,
            &home,
            &spec("pessoal", "x", AgentKind::Shell),
            1,
            &options,
        )
        .unwrap();
        assert_eq!(shell.argv, ["bash", "-l"]);
    }

    #[test]
    fn full_network_drops_forward_ports_and_host_filter() {
        let (config, home) = setup();
        let options = with_settings(ProjectSettings {
            full_network: true,
            allow_hosts: vec!["pypi.org".into()],
            ..Default::default()
        });
        let plan = LaunchPlan::build_with(
            &config,
            &home,
            &spec("pessoal", "x", AgentKind::Opencode),
            1,
            &options,
        )
        .unwrap();
        let argv = plan.argv.join(" ");
        assert!(argv.contains(" --network "), "{argv}");
        assert!(!argv.contains("--forward-port"), "{argv}");
        assert!(!argv.contains("--allow-host"), "{argv}");
    }

    #[test]
    fn settings_file_is_outside_every_jail_mount() {
        let (config, home) = setup();
        let file = crate::project_settings::settings_path(&home);
        for agent in [AgentKind::Claude, AgentKind::Opencode] {
            let plan =
                LaunchPlan::build(&config, &home, &spec("pessoal", "game", agent), 1).unwrap();
            let mounts = plan
                .argv
                .windows(2)
                .filter(|w| matches!(w[0].as_str(), "--map" | "--rw-map" | "--claude-dir"));
            for pair in mounts {
                assert!(
                    !file.starts_with(&pair[1]),
                    "{} expõe {}",
                    pair[1],
                    file.display()
                );
            }
            assert!(!file.starts_with(&plan.cwd));
        }
    }

    /// Roda ai-jail real (devenv/VM): só o `--rw-map` do perfil ativo enxerga credenciais.
    #[test]
    fn jail_hides_other_profile_data_dirs() {
        let home = scratch("jail-perfis");
        let pessoal_data = profile_data_dir(&home, "pessoal");
        let trabalho_data = profile_data_dir(&home, "trabalho");
        fs::create_dir_all(pessoal_data.join("claude")).unwrap();
        fs::create_dir_all(trabalho_data.join("claude")).unwrap();
        fs::write(pessoal_data.join("isolado"), "ok").unwrap();
        fs::write(trabalho_data.join("secreto"), "não").unwrap();
        fs::create_dir_all(home.join(".local/lib/ai-memory")).unwrap();
        fs::create_dir_all(home.join(".local/share/ai-memory")).unwrap();

        let projects = home.join("projetos");
        let game = projects.join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(
            projects.join(".ai-memory.toml"),
            "workspace = \"pessoal\"\n",
        )
        .unwrap();

        let text = EXAMPLE.replace(
            "root = \"~/Documentos/pessoal/projetos\"",
            &format!(r#"root = "{}""#, projects.display()),
        );
        let config = Config::parse(&text, &home).unwrap();
        let plan = LaunchPlan::build(
            &config,
            &home,
            &spec("pessoal", "game", AgentKind::Claude),
            1,
        )
        .unwrap();

        let other = trabalho_data.display();
        let own = pessoal_data.join("isolado").display().to_string();
        let script =
            format!("test -f '{own}' && test ! -e '{other}' && test ! -e '{other}/secreto'");
        run_in_jail(&plan, &script).unwrap_or_else(|e| panic!("isolamento por perfil: {e}"));

        fs::remove_dir_all(&home).unwrap();
    }

    fn run_in_jail(plan: &LaunchPlan, script: &str) -> Result<(), String> {
        let mut argv = plan.argv.clone();
        let sep = argv
            .iter()
            .position(|a| a == "--")
            .ok_or("LaunchPlan sem --")?;
        argv.truncate(sep + 1);
        argv.extend(["bash".into(), "-c".into(), script.into()]);
        let status = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&plan.cwd)
            .envs(plan.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .status()
            .map_err(|e| format!("ai-jail ausente ou não executável: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("ai-jail saiu com {status}"))
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
