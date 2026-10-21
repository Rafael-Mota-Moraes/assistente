//! Como uma sessão de agente é lançada na VM: tmux → ai-jail → claude | opencode. A aba
//! Terminal (`AgentKind::Shell`) é a exceção: bash direto no tmux, fora do jail (F9.1).
//! Spec: docs/specs/02-hubd-e-sessoes.md e docs/specs/05-sandbox.md

use crate::config::{Config, Profile, is_valid_id};
use crate::project_settings::ProjectSettings;
use crate::sleep::jail_config_at;
use crate::worker::WorkerRun;
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
    /// Acesso à API dos agentes (F9.5); `None` = a sessão não pode pedir nada fora do jail.
    pub exec: Option<ExecAccess>,
}

/// Como o agente pede para rodar um comando fora do jail (F9.5): porta do `agent_listen` e
/// o token desta sessão (vai por env, nunca na linha de comando).
#[derive(Debug, Clone)]
pub struct ExecAccess {
    pub port: u16,
    pub token: String,
}

/// O Claude do perfil tem login? O `assistente login claude <perfil>` grava o
/// `.credentials.json` no `CLAUDE_CONFIG_DIR` do perfil; sem ele o Claude abre e morre na hora
/// dentro do jail. Perfil com `claude_via_9router` não usa login da Anthropic.
pub fn claude_logged_in(config: &Config, home: &Path, profile_id: &str) -> bool {
    let Some(profile) = config.profile(profile_id) else {
        return false;
    };
    profile.claude_via_9router
        || profile_data_dir(home, profile_id)
            .join("claude/.credentials.json")
            .is_file()
}

/// Onde mora o cliente `fora-do-jail` na VM (instalado por `vm/provisionar.sh`); entra no
/// jail só leitura.
pub fn exec_client_dir(home: &Path) -> PathBuf {
    home.join(".local/lib/assistente")
}

/// O jail efetivo: configurações do projeto + `[jail]` do `.assistente.toml` (compatibilidade).
struct JailOptions<'a> {
    docker: bool,
    full_network: bool,
    allow_hosts: &'a [String],
    exec_port: Option<u16>,
    /// Comando que roda dentro do jail (o agente, ou o `trabalhador`).
    command: &'a [String],
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
        Self::assemble(config, home, spec, seq, options, None)
    }

    /// Sessão de trabalhador (F11.6): o opencode roda `vm/trabalhador` no worktree da tarefa,
    /// no mesmo jail de um agente. Os arquivos da execução ficam em `worker::files`.
    pub fn build_worker(
        config: &Config,
        home: &Path,
        spec: &SessionSpec,
        seq: u32,
        options: &LaunchOptions,
        run: &WorkerRun,
    ) -> Result<Self, LaunchError> {
        Self::assemble(config, home, spec, seq, options, Some(run))
    }

    fn assemble(
        config: &Config,
        home: &Path,
        spec: &SessionSpec,
        seq: u32,
        options: &LaunchOptions,
        worker: Option<&WorkerRun>,
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
        let session = tmux_session_name(&spec.profile, &spec.project, spec.agent, seq);
        let mut env = profile_env(config, profile, &data_dir);
        let exec = options
            .exec
            .as_ref()
            .filter(|_| spec.agent != AgentKind::Shell);
        if let Some(exec) = exec {
            env.push((
                "ASSISTENTE_EXEC_URL".to_owned(),
                format!("http://127.0.0.1:{}", exec.port),
            ));
            env.push(("ASSISTENTE_EXEC_TOKEN".to_owned(), exec.token.clone()));
            env.push((
                "ASSISTENTE_FORA_DO_JAIL".to_owned(),
                exec_client_dir(home)
                    .join("fora-do-jail")
                    .display()
                    .to_string(),
            ));
            if spec.agent == AgentKind::Claude {
                // O pedido espera a sua aprovação: o Bash do Claude corta em 2 min por padrão
                // e aceita até este teto quando o agente pede um timeout maior.
                env.push(("BASH_MAX_TIMEOUT_MS".to_owned(), "1800000".to_owned()));
            }
        }
        let mut cwd = profile.root.join(&spec.project);
        let argv = if spec.agent == AgentKind::Shell {
            // Login: pega o PATH do usuário (~/.local/bin) como num terminal comum.
            vec!["bash".to_owned(), "-l".to_owned()]
        } else {
            let command = match worker {
                Some(run) => {
                    cwd = run.worktree.clone();
                    let files = crate::worker::files(&data_dir, &session);
                    env.extend(worker_env(run, &files));
                    vec![
                        exec_client_dir(home)
                            .join("trabalhador")
                            .display()
                            .to_string(),
                    ]
                }
                None => agent_command(spec.agent),
            };
            let settings = &options.settings;
            let jail = JailOptions {
                docker: settings.docker || jail_config_at(&cwd).docker,
                full_network: settings.full_network,
                allow_hosts: &settings.allow_hosts,
                exec_port: exec.map(|e| e.port),
                command: &command,
            };
            jail_argv(config, home, profile, &data_dir, &env, spec.agent, &jail)
        };
        Ok(Self {
            tmux_session: session,
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
        // API dos agentes (F9.5): só esta porta do hubd entra; a 7878 (app) continua fora.
        if let Some(port) = jail.exec_port {
            argv.push("--forward-port".to_owned());
            argv.push(port.to_string());
        }
    }
    if jail.exec_port.is_some() {
        argv.push("--map".to_owned());
        argv.push(exec_client_dir(home).display().to_string());
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
    argv.extend(jail.command.iter().cloned());
    argv
}

/// Comando do agente dentro do jail. O opencode 2 roda como cliente de um servidor em segundo
/// plano compartilhado, que não sobrevive ao jail: com `--standalone` cada sessão sobe o seu.
fn agent_command(agent: AgentKind) -> Vec<String> {
    let mut command = vec![agent.id().to_owned()];
    if agent == AgentKind::Opencode {
        command.push("--standalone".to_owned());
    }
    command
}

/// Variáveis que o `vm/trabalhador` lê (env do tmux, passadas ao jail com `--env`).
fn worker_env(run: &WorkerRun, files: &crate::worker::WorkerFiles) -> Vec<(String, String)> {
    vec![
        ("AGENTE_TAREFA".to_owned(), run.task.clone()),
        ("AGENTE_MODELOS".to_owned(), run.models.join(" ")),
        ("AGENTE_TENTATIVAS".to_owned(), run.attempts.to_string()),
        ("AGENTE_MAX_MIN".to_owned(), run.max_minutes.to_string()),
        ("AGENTE_ESPERA".to_owned(), run.wait_secs.to_string()),
        (
            "AGENTE_PROMPT_FILE".to_owned(),
            files.prompt.display().to_string(),
        ),
        ("AGENTE_LOG".to_owned(), files.log.display().to_string()),
    ]
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
    fn claude_login_is_per_profile() {
        let home = scratch("login-claude");
        let text = EXAMPLE.replacen("claude_via_9router = false", "claude_via_9router = true", 1);
        let config = Config::parse(&text, &home).unwrap();
        // pessoal passa pelo 9router: não precisa de login.
        assert!(claude_logged_in(&config, &home, "pessoal"));
        assert!(!claude_logged_in(&config, &home, "trabalho"));
        let claude = profile_data_dir(&home, "trabalho").join("claude");
        fs::create_dir_all(&claude).unwrap();
        fs::write(claude.join(".credentials.json"), "{}").unwrap();
        assert!(claude_logged_in(&config, &home, "trabalho"));
        assert!(!claude_logged_in(&config, &home, "nope"));
        fs::remove_dir_all(home).unwrap();
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
        LaunchOptions {
            settings,
            exec: None,
        }
    }

    #[test]
    fn worker_runs_the_trabalhador_in_the_worktree_inside_the_jail() {
        let (config, home) = setup();
        let options = LaunchOptions {
            settings: ProjectSettings::default(),
            exec: Some(ExecAccess {
                port: 7879,
                token: "f0f0segredo".into(),
            }),
        };
        let run = WorkerRun {
            task: "T7".into(),
            models: vec!["x/a".into(), "x/b".into()],
            attempts: 2,
            max_minutes: 5,
            wait_secs: 0,
            worktree: PathBuf::from(
                "/home/rafa/Documentos/pessoal/projetos/x/.assistente/worktrees/T7",
            ),
        };
        let plan = LaunchPlan::build_worker(
            &config,
            &home,
            &spec("pessoal", "x", AgentKind::Opencode),
            1,
            &options,
            &run,
        )
        .unwrap();
        assert_eq!(
            plan.cwd,
            PathBuf::from("/home/rafa/Documentos/pessoal/projetos/x/.assistente/worktrees/T7")
        );
        assert_eq!(plan.tmux_session, "pessoal--x--opencode-1");
        assert_eq!(
            plan.argv.last().map(String::as_str),
            Some("/home/rafa/.local/lib/assistente/trabalhador")
        );
        assert!(!plan.argv.iter().any(|arg| arg == "--standalone"));
        assert_eq!(env(&plan, "AGENTE_TAREFA"), Some("T7"));
        assert_eq!(env(&plan, "AGENTE_MODELOS"), Some("x/a x/b"));
        assert_eq!(env(&plan, "AGENTE_TENTATIVAS"), Some("2"));
        assert_eq!(
            env(&plan, "AGENTE_PROMPT_FILE"),
            Some("/home/rafa/.assistente/perfis/pessoal/trabalhadores/pessoal--x--opencode-1.md")
        );
        assert_eq!(
            env(&plan, "AGENTE_LOG"),
            Some("/home/rafa/.assistente/perfis/pessoal/trabalhadores/pessoal--x--opencode-1.log")
        );
        // Mesmo jail: o env do trabalhador passa com `--env`, e o token continua fora da linha.
        let argv = plan.argv.join(" ");
        assert!(argv.contains("--env AGENTE_MODELOS "), "{argv}");
        assert!(!argv.contains("f0f0segredo"), "o token vazou: {argv}");
    }

    #[test]
    fn exec_access_forwards_only_the_agent_port_and_hides_the_token() {
        let (config, home) = setup();
        let options = LaunchOptions {
            settings: ProjectSettings::default(),
            exec: Some(ExecAccess {
                port: 7879,
                token: "f0f0segredo".into(),
            }),
        };
        let build = |agent| {
            LaunchPlan::build_with(&config, &home, &spec("pessoal", "x", agent), 1, &options)
                .unwrap()
        };
        let claude = build(AgentKind::Claude);
        let argv = claude.argv.join(" ");
        assert!(argv.contains("--forward-port 7879"), "{argv}");
        assert!(!argv.contains("7878"), "{argv}");
        assert!(
            argv.contains("--map /home/rafa/.local/lib/assistente"),
            "{argv}"
        );
        assert!(!argv.contains("f0f0segredo"), "o token vazou: {argv}");
        assert!(argv.contains("--env ASSISTENTE_EXEC_TOKEN "), "{argv}");
        assert_eq!(env(&claude, "ASSISTENTE_EXEC_TOKEN"), Some("f0f0segredo"));
        assert_eq!(
            env(&claude, "ASSISTENTE_EXEC_URL"),
            Some("http://127.0.0.1:7879")
        );
        assert_eq!(env(&claude, "BASH_MAX_TIMEOUT_MS"), Some("1800000"));
        let opencode = build(AgentKind::Opencode);
        assert_eq!(env(&opencode, "BASH_MAX_TIMEOUT_MS"), None);
        assert!(opencode.argv.join(" ").contains("--forward-port 7879"));
        // A aba Terminal é você digitando: sem token.
        let shell = build(AgentKind::Shell);
        assert_eq!(env(&shell, "ASSISTENTE_EXEC_TOKEN"), None);
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
