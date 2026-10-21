//! Sessões de agente = sessões do tmux (F2.2). O hubd não guarda estado: perfil, projeto e
//! agente ficam em opções do próprio tmux (`@assistente_*`), então a lista sobrevive a um
//! restart do hubd. Spec: docs/specs/02-hubd-e-sessoes.md

use hub_core::{AgentKind, LaunchPlan, SessionSpec};
use serde::Serialize;
use std::path::PathBuf;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SessionInfo {
    /// Nome da sessão no tmux.
    pub id: String,
    pub profile: String,
    pub project: String,
    pub agent: AgentKind,
    pub created_at: u64,
    /// Quantos clientes (abas) estão anexados agora.
    pub attached: u32,
}

/// Acesso ao tmux. `socket` isola um servidor tmux próprio (usado nos testes).
#[derive(Debug, Clone)]
pub struct Tmux {
    socket: Option<PathBuf>,
}

impl Default for Tmux {
    fn default() -> Self {
        Self::new(None)
    }
}

/// O tmux troca TAB e outros caracteres de controle por `_` na saída: separador visível.
const SEP: &str = "|~|";
const FORMAT: &str = "#{session_name}|~|#{@assistente_profile}|~|#{@assistente_project}|~|#{@assistente_agent}|~|#{session_created}|~|#{session_attached}";

impl Tmux {
    pub fn new(socket: Option<PathBuf>) -> Self {
        Self { socket }
    }

    /// `tmux [-S socket] <args…>` pronto para rodar (o hubd e o terminal usam o mesmo).
    pub fn command(&self, args: &[String]) -> Command {
        let mut command = Command::new("tmux");
        if let Some(socket) = &self.socket {
            command.arg("-S").arg(socket);
        }
        command.args(args).kill_on_drop(true);
        command
    }

    pub fn std_args(&self) -> Vec<String> {
        match &self.socket {
            Some(socket) => vec!["-S".to_owned(), socket.display().to_string()],
            None => Vec::new(),
        }
    }

    async fn run(&self, args: &[String]) -> Result<String, String> {
        let output = self
            .command(args)
            .output()
            .await
            .map_err(|e| format!("não consegui rodar tmux: {e}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    }

    /// Só as sessões criadas pelo Assistente (as outras do tmux ficam de fora).
    pub async fn list(&self) -> Result<Vec<SessionInfo>, String> {
        let out = match self.run(&args(&["list-sessions", "-F", FORMAT])).await {
            Ok(out) => out,
            // Sem servidor tmux = nenhuma sessão.
            Err(e) if no_server(&e) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut sessions: Vec<SessionInfo> = out.lines().filter_map(parse_line).collect();
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(sessions)
    }

    pub async fn find(&self, id: &str) -> Result<Option<SessionInfo>, String> {
        Ok(self.list().await?.into_iter().find(|s| s.id == id))
    }

    /// Cria a sessão desanexada e grava os metadados nela. `exec_token` (F9.5) fica numa
    /// opção do tmux: some junto com a sessão e nunca vai para o `SessionInfo`.
    pub async fn create(
        &self,
        plan: &LaunchPlan,
        spec: &SessionSpec,
        exec_token: Option<&str>,
    ) -> Result<SessionInfo, String> {
        self.run(&plan.tmux_new_session_argv()[1..]).await?;
        // `set-option -t` espera um painel: `=nome:` é "exatamente esta sessão" (`=nome` falha).
        let target = format!("={}:", plan.tmux_session);
        if let Some(token) = exec_token {
            self.run(&args(&[
                "set-option",
                "-t",
                &target,
                "@assistente_exec_token",
                token,
            ]))
            .await
            .map_err(|e| format!("a sessão terminou logo ao iniciar ({e})"))?;
        }
        for (key, value) in [
            ("@assistente_profile", spec.profile.as_str()),
            ("@assistente_project", spec.project.as_str()),
            ("@assistente_agent", spec.agent.id()),
            // As abas do app substituem a barra do tmux; mouse liga rolagem e clique nos TUIs.
            ("status", "off"),
            ("mouse", "on"),
        ] {
            self.run(&args(&["set-option", "-t", &target, key, value]))
                .await
                .map_err(|e| format!("a sessão terminou logo ao iniciar ({e})"))?;
        }
        // Opção do servidor. TUIs que prendem o mouse (opencode) copiam por OSC 52; no padrão
        // (`external`) o tmux engole a sequência e ela não chega ao app.
        self.run(&args(&["set-option", "-s", "set-clipboard", "on"]))
            .await?;
        self.find(&plan.tmux_session)
            .await?
            .ok_or_else(|| "a sessão terminou logo ao iniciar".to_owned())
    }

    /// Sessão dona do token da API dos agentes (F9.5). Sessão encerrada = token inválido.
    pub async fn session_for_exec_token(&self, token: &str) -> Result<Option<SessionInfo>, String> {
        if token.is_empty() {
            return Ok(None);
        }
        let format = format!("#{{session_name}}{SEP}#{{@assistente_exec_token}}");
        let out = match self.run(&args(&["list-sessions", "-F", &format])).await {
            Ok(out) => out,
            Err(e) if no_server(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
        let owner = out.lines().find_map(|line| {
            let (id, stored) = line.split_once(SEP)?;
            constant_time_eq(stored.as_bytes(), token.as_bytes()).then(|| id.to_owned())
        });
        match owner {
            Some(id) => self.find(&id).await,
            None => Ok(None),
        }
    }

    pub async fn kill(&self, id: &str) -> Result<(), String> {
        self.run(&args(&["kill-session", "-t", &format!("={id}")]))
            .await
            .map(|_| ())
    }
}

/// O erro do tmux que quer dizer "não há servidor": nenhum, ou um que acabou de encerrar (o
/// socket ainda existe e recusa conexão, o que acontece ao matar a última sessão).
fn no_server(error: &str) -> bool {
    [
        "no server running",
        "No such file",
        "Connection refused",
        "server exited unexpectedly",
    ]
    .iter()
    .any(|text| error.contains(text))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Token novo da API dos agentes: 32 bytes do kernel em hex.
pub fn new_exec_token() -> std::io::Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn parse_line(line: &str) -> Option<SessionInfo> {
    let mut parts = line.split(SEP);
    let id = parts.next()?.to_owned();
    let profile = parts.next()?.to_owned();
    let project = parts.next()?.to_owned();
    // Sessão que não é do Assistente: sem agente conhecido.
    let agent = AgentKind::from_id(parts.next()?)?;
    Some(SessionInfo {
        id,
        profile,
        project,
        agent,
        created_at: parts.next()?.parse().ok()?,
        attached: parts.next()?.parse().ok()?,
    })
}

/// Menor `n` livre para (perfil, projeto, agente): claude-1, claude-2…
pub fn next_seq(existing: &[SessionInfo], spec: &SessionSpec) -> u32 {
    let taken = |n: u32| {
        existing.iter().any(|s| {
            s.profile == spec.profile
                && s.project == spec.project
                && s.agent == spec.agent
                && s.id.ends_with(&format!("-{n}"))
        })
    };
    (1..).find(|&n| !taken(n)).unwrap_or(1)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;

    /// tmux isolado num socket temporário; mata o servidor ao sair.
    pub struct TestTmux {
        pub tmux: Tmux,
        pub dir: PathBuf,
    }

    impl TestTmux {
        pub fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("hubd-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self {
                tmux: Tmux::new(Some(dir.join("tmux.sock"))),
                dir,
            }
        }

        /// Plano "falso": roda `bash -c <script>` em vez do ai-jail + agente.
        pub fn plan(&self, name: &str, script: &str) -> LaunchPlan {
            LaunchPlan {
                tmux_session: name.to_owned(),
                cwd: self.dir.clone(),
                env: vec![("ASSISTENTE_TESTE".into(), "sim".into())],
                argv: vec!["bash".into(), "-c".into(), script.into()],
            }
        }
    }

    impl Drop for TestTmux {
        fn drop(&mut self) {
            let _ = std::process::Command::new("tmux")
                .arg("-S")
                .arg(self.dir.join("tmux.sock"))
                .arg("kill-server")
                .stderr(std::process::Stdio::null())
                .status();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    pub fn spec(project: &str, agent: AgentKind) -> SessionSpec {
        SessionSpec {
            profile: "pessoal".into(),
            project: project.into(),
            agent,
        }
    }

    #[tokio::test]
    async fn empty_server_lists_nothing() {
        let t = TestTmux::new("vazio");
        assert_eq!(t.tmux.list().await.unwrap(), []);
    }

    #[tokio::test]
    async fn create_list_and_kill() {
        let t = TestTmux::new("ciclo");
        let spec = spec("Meu.App", AgentKind::Opencode);
        let info = t
            .tmux
            .create(
                &t.plan("pessoal--meu-app--opencode-1", "sleep 30"),
                &spec,
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            info.project, "Meu.App",
            "nome original vem das opções do tmux"
        );
        assert_eq!(info.agent, AgentKind::Opencode);
        assert_eq!(info.attached, 0);

        // Uma sessão alheia ao Assistente não aparece.
        t.tmux
            .run(&args(&["new-session", "-d", "-s", "outra", "sleep 30"]))
            .await
            .unwrap();
        assert_eq!(t.tmux.list().await.unwrap(), std::slice::from_ref(&info));

        t.tmux.kill(&info.id).await.unwrap();
        assert_eq!(t.tmux.list().await.unwrap(), []);
    }

    #[tokio::test]
    async fn env_and_cwd_reach_the_command() {
        let t = TestTmux::new("env");
        let out = t.dir.join("saida.txt");
        let script = format!(
            "echo \"$ASSISTENTE_TESTE $PWD\" > {}; sleep 30",
            out.display()
        );
        t.tmux
            .create(
                &t.plan("pessoal--x--claude-1", &script),
                &spec("x", AgentKind::Claude),
                None,
            )
            .await
            .unwrap();
        let text = wait_for_file(&out).await;
        assert_eq!(text.trim(), format!("sim {}", t.dir.display()));
    }

    #[tokio::test]
    async fn command_that_exits_at_once_is_an_error() {
        let t = TestTmux::new("morre");
        let err = t
            .tmux
            .create(
                &t.plan("pessoal--x--claude-1", "exit 0"),
                &spec("x", AgentKind::Claude),
                None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("terminou logo ao iniciar"), "{err}");
    }

    #[tokio::test]
    async fn exec_token_finds_its_session_and_dies_with_it() {
        let t = TestTmux::new("token");
        let token = new_exec_token().unwrap();
        assert_eq!(token.len(), 64);
        assert_ne!(token, new_exec_token().unwrap());
        let info = t
            .tmux
            .create(
                &t.plan("pessoal--x--claude-1", "sleep 30"),
                &spec("x", AgentKind::Claude),
                Some(&token),
            )
            .await
            .unwrap();
        t.tmux
            .create(
                &t.plan("pessoal--x--claude-2", "sleep 30"),
                &spec("x", AgentKind::Claude),
                None,
            )
            .await
            .unwrap();
        let found = t.tmux.session_for_exec_token(&token).await.unwrap();
        assert_eq!(
            found.as_ref().map(|s| s.id.as_str()),
            Some("pessoal--x--claude-1")
        );
        assert_eq!(t.tmux.session_for_exec_token("errado").await.unwrap(), None);
        // Sessão sem token não casa com token vazio.
        assert_eq!(t.tmux.session_for_exec_token("").await.unwrap(), None);
        // O segredo não vai para a lista que a UI recebe.
        let listed = serde_json::to_string(&t.tmux.list().await.unwrap()).unwrap();
        assert!(!listed.contains(&token), "{listed}");

        t.tmux.kill(&info.id).await.unwrap();
        assert_eq!(t.tmux.session_for_exec_token(&token).await.unwrap(), None);
    }

    #[test]
    fn next_seq_fills_the_first_gap() {
        let mk = |id: &str| SessionInfo {
            id: id.into(),
            profile: "pessoal".into(),
            project: "x".into(),
            agent: AgentKind::Claude,
            created_at: 0,
            attached: 0,
        };
        let s = spec("x", AgentKind::Claude);
        assert_eq!(next_seq(&[], &s), 1);
        let existing = [mk("pessoal--x--claude-1"), mk("pessoal--x--claude-3")];
        assert_eq!(next_seq(&existing, &s), 2);
        assert_eq!(next_seq(&existing, &spec("x", AgentKind::Opencode)), 1);
        assert_eq!(next_seq(&existing, &spec("x", AgentKind::Shell)), 1);
    }

    #[test]
    fn parse_line_knows_the_terminal_tab_and_skips_foreign_sessions() {
        let info = parse_line("pessoal--x--shell-1|~|pessoal|~|x|~|shell|~|10|~|1").unwrap();
        assert_eq!(info.agent, AgentKind::Shell);
        assert_eq!(info.attached, 1);
        assert!(parse_line("minha|~||~||~||~|10|~|0").is_none());
    }

    pub async fn wait_for_file(path: &Path) -> String {
        for _ in 0..50 {
            if let Ok(text) = std::fs::read_to_string(path)
                && !text.is_empty()
            {
                return text;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("{} não apareceu", path.display());
    }
}
