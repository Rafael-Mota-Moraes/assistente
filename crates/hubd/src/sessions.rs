//! Sessões de agente = sessões do tmux (F2.2). O hubd não guarda estado: perfil, projeto e
//! agente ficam em opções do próprio tmux (`@assistente_*`), então a lista sobrevive a um
//! restart do hubd. Spec: docs/specs/02-hubd-e-sessoes.md

use hub_core::{AgentKind, LaunchPlan, SessionSpec};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

/// Uma sessão e o estado do seu painel. Com `remain-on-exit` (F14.3) a sessão continua viva
/// depois que o processo morre: a saída e o status de saída ficam visíveis na aba até o
/// usuário encerrar a sessão.
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
    /// O processo do painel já saiu: a sessão existe, mas não trabalha (F14.3/F14.4).
    /// A UI usa isto para marcar a aba como morta sem abrir o terminal.
    pub dead: bool,
}

/// O painel de uma sessão (F14.3): o processo vive, ou já morreu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneState {
    /// O processo do painel ainda roda.
    Alive,
    /// O processo morreu. O status é o código de saída, quando deu para saber (o tmux às
    /// vezes perde o SIGCHLD do painel morto e o status não aparece — ver `Tmux::reap`).
    Dead(Option<i32>),
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
/// `pane_dead` no fim: o painel morreu? (F14.3, `remain-on-exit`).
const FORMAT: &str = "#{session_name}|~|#{@assistente_profile}|~|#{@assistente_project}|~|#{@assistente_agent}|~|#{session_created}|~|#{session_attached}|~|#{pane_dead}";

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

    /// Só as sessões cujo processo ainda vive: as mortas continuam no tmux (F14.3), mas não
    /// seguram tarefa no quadro nem token da API dos agentes.
    pub async fn list_running(&self) -> Result<Vec<SessionInfo>, String> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .filter(|session| !session.dead)
            .collect())
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
        // F14.3/F14.4: `remain-on-exit` na janela da sessão só para agentes interativos
        // (Claude, OpenCode): o painel fica com a saída e o status de saída. O trabalhador e
        // o Terminal não usam: o trabalhador que sai com 0 fecha a própria sessão e o Terminal
        // fecha no `exit`. O trabalhador que falha pede o painel de volta (`keep_panel`).
        let mut argv = plan.tmux_new_session_argv();
        argv.remove(0); // o "tmux" do começo
        if plan.remain_on_exit {
            argv.extend([
                ";".to_owned(),
                "set-option".to_owned(),
                "-w".to_owned(),
                "-t".to_owned(),
                format!("={}:", plan.tmux_session),
                "remain-on-exit".to_owned(),
                "on".to_owned(),
            ]);
        }
        if let Err(e) = self.run(&argv).await {
            // A sessão não existe: o processo morreu antes do `set-option` (sem
            // `remain-on-exit` o painel leva a sessão junto). Senão é o tmux que recusou.
            let sumiu = matches!(self.find(&plan.tmux_session).await, Ok(None));
            return Err(if sumiu {
                format!("a sessão terminou logo ao iniciar ({e})")
            } else {
                e
            });
        }
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

    /// Deixa o painel da sessão na tela depois que o processo morre (F14.4).
    ///
    /// O trabalhador que desiste da tarefa chama isto (pelo `release`) antes de morrer: sem o
    /// `remain-on-exit` a sessão morre junto com o processo e o erro some com ela. Só a janela
    /// da sessão: o servidor tmux do usuário continua sem a opção.
    pub async fn keep_panel(&self, id: &str) -> Result<(), String> {
        self.run(&args(&[
            "set-option",
            "-w",
            "-t",
            &format!("={id}:"),
            "remain-on-exit",
            "on",
        ]))
        .await
        .map(|_| ())
    }

    /// Sessão dona do token da API dos agentes (F9.5). Sessão encerrada = token inválido:
    /// com o `remain-on-exit` (F14.3) a sessão morta continua no tmux, mas o token dela não
    /// vale mais (o processo que o usava já foi embora).
    pub async fn session_for_exec_token(&self, token: &str) -> Result<Option<SessionInfo>, String> {
        if token.is_empty() {
            return Ok(None);
        }
        let format =
            format!("#{{session_name}}{SEP}#{{@assistente_exec_token}}{SEP}#{{pane_dead}}");
        let out = match self.run(&args(&["list-sessions", "-F", &format])).await {
            Ok(out) => out,
            Err(e) if no_server(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
        let owner = out.lines().find_map(|line| {
            let mut parts = line.split(SEP);
            let id = parts.next()?;
            let stored = parts.next()?;
            let dead = parts.next().unwrap_or("0") == "1";
            if dead {
                return None;
            }
            constant_time_eq(stored.as_bytes(), token.as_bytes()).then(|| id.to_owned())
        });
        match owner {
            Some(id) => self.find(&id).await,
            None => Ok(None),
        }
    }

    /// O que o tmux sabe do painel: o processo morreu? e com que status de saída?
    async fn pane_info(&self, id: &str) -> Option<(bool, Option<i32>)> {
        let out = self
            .run(&args(&[
                "list-panes",
                "-t",
                &format!("={id}:"),
                "-F",
                "#{pane_dead}:#{pane_dead_status}",
            ]))
            .await
            .ok()?;
        let line = out.lines().next()?;
        let (dead, status) = line.split_once(':')?;
        // O status vem vazio quando o tmux não sabe o código de saída (o painel morreu por
        // sinal, ou o tmux perdeu o SIGCHLD — ver `reap`).
        Some((dead == "1", status.trim().parse().ok()))
    }

    /// O painel da sessão (F14.3): o processo vive, ou já morreu.
    pub async fn pane_state(&self, id: &str) -> PaneState {
        let Some((dead, status)) = self.pane_info(id).await else {
            return PaneState::Alive;
        };
        if !dead {
            return PaneState::Alive;
        }
        if let Some(status) = status {
            return PaneState::Dead(Some(status));
        }
        // O tmux perdeu o SIGCHLD do painel morto: dá para recuperar (ver `reap`).
        self.reap(id).await;
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if let Some((_, Some(status))) = self.pane_info(id).await {
                return PaneState::Dead(Some(status));
            }
        }
        PaneState::Dead(None)
    }

    /// Faz o servidor do tmux recolher um zumbi esquecido (F14.3).
    ///
    /// O tmux 3.4 perde o SIGCHLD do painel morto em ~5% das vezes: o processo fica zumbi e
    /// o `pane_dead_status` não aparece nunca. O `waitpid` do servidor recolhe todos os
    /// filhos que morrem, então um `run-shell` (que morre na hora) faz ele recolher o zumbi
    /// esquecido — e o status de saída aparece.
    async fn reap(&self, id: &str) {
        if let Err(e) = self.run(&args(&["run-shell", "-b", "true"])).await {
            debug!("não consegui reap ar o painel {id}: {e}");
        }
    }

    /// O que o processo imprimiu no painel (a tela e o histórico), para o terminal mostrar a
    /// saída de uma sessão morta (F14.3). O tmux rola o painel uma linha quando o processo
    /// morre: por isso vem o histórico, e não só a tela.
    pub async fn capture(&self, id: &str) -> Option<String> {
        let out = self.capture_pane(id, -500).await.ok()?;
        // O terminal do app espera \r\n.
        Some(out.replace('\n', "\r\n"))
    }

    pub async fn kill(&self, id: &str) -> Result<(), String> {
        self.run(&args(&["kill-session", "-t", &format!("={id}")]))
            .await
            .map(|_| ())
    }

    /// A saída do painel de uma sessão (o que ela imprimiu). `lines` é quantas linhas de
    /// histórico entram (`-200` = as últimas 200).
    pub async fn capture_pane(&self, id: &str, lines: i32) -> Result<String, String> {
        // O alvo do `capture-pane` é um painel: `=nome:` é "exatamente esta sessão" (`=nome`
        // dá "can't find pane").
        let target = format!("={id}:");
        self.run(&args(&[
            "capture-pane",
            "-p",
            "-t",
            &target,
            "-S",
            &lines.to_string(),
        ]))
        .await
    }

    /// Cola `text` no prompt da sessão, **sem Enter** (F12.4).
    ///
    /// `set-buffer` + `paste-buffer -r`: o Enter é do usuário, para não aprovar uma pergunta
    /// pendente do agente nem executar texto no shell. O texto vai em uma linha só (uma
    /// quebra no buffer entra como Enter).
    pub async fn paste(&self, id: &str, text: &str) -> Result<(), String> {
        let line = one_line(text);
        if line.is_empty() {
            return Err("o texto para colar está vazio".into());
        }
        let pane = self
            .pane_of(id)
            .await
            .map_err(|e| format!("a sessão {id} não está viva: {e}"))?;
        // O buffer tem nome: o buffer de cópia do usuário não é mexido.
        self.run(&args(&["set-buffer", "-b", BUFFER, "--", &line]))
            .await?;
        self.run(&args(&[
            "paste-buffer",
            "-b",
            BUFFER,
            "-p",
            "-r",
            "-t",
            &pane,
        ]))
        .await
        .map(|_| ())
    }

    /// O painel da sessão (é nele que o agente roda).
    async fn pane_of(&self, id: &str) -> Result<String, String> {
        let out = self
            .run(&args(&[
                "list-panes",
                // `-s`: todas as janelas da sessão, não só a atual.
                "-s",
                "-t",
                &format!("={id}"),
                "-F",
                "#{pane_id}",
            ]))
            .await?;
        out.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("a sessão {id} não tem painel"))
    }
}

/// O buffer do tmux onde o pick vai antes de ser colado.
const BUFFER: &str = "assistente-pick";

/// Uma linha só: no tmux, uma quebra no buffer vale como Enter (F12.4).
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
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
        // O painel morreu? (F14.3: a sessão continua viva, mas o processo foi embora.)
        dead: parts.next().unwrap_or("0") == "1",
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

        /// Plano "falso": roda `bash -c <script>` no lugar do ai-jail + agente.
        /// É a sessão de agente: o painel fica na tela quando o processo morre (F14.3).
        pub fn plan(&self, name: &str, script: &str) -> LaunchPlan {
            self.plan_with(name, script, true)
        }

        /// O mesmo plano de uma sessão de trabalhador: sem `remain-on-exit`, a sessão morre
        /// junto com o processo (F14.4).
        pub fn plan_worker(&self, name: &str, script: &str) -> LaunchPlan {
            self.plan_with(name, script, false)
        }

        fn plan_with(&self, name: &str, script: &str, remain_on_exit: bool) -> LaunchPlan {
            LaunchPlan {
                tmux_session: name.to_owned(),
                cwd: self.dir.clone(),
                env: vec![("ASSISTENTE_TESTE".into(), "sim".into())],
                argv: vec!["bash".into(), "-c".into(), script.into()],
                remain_on_exit,
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

    /// F14.3: o processo que morre deixa o painel na tela, com a saída e o status de saída.
    #[tokio::test]
    async fn command_that_exits_at_once_leaves_the_dead_pane() {
        let t = TestTmux::new("morre");
        let info = t
            .tmux
            .create(
                &t.plan("pessoal--x--claude-1", "echo 'claude: not found'; exit 127"),
                &spec("x", AgentKind::Claude),
                None,
            )
            .await
            .unwrap();
        assert_eq!(info.id, "pessoal--x--claude-1");
        // A sessão continua viva (o painel fica), mas o processo morreu.
        assert_eq!(
            t.tmux.pane_state(&info.id).await,
            PaneState::Dead(Some(127))
        );
        let listed = t.tmux.list().await.unwrap();
        assert!(listed[0].dead, "a sessão morta tem de aparecer como morta");
        // A saída do processo continua visível no painel.
        let pane = t
            .tmux
            .run(&args(&[
                "capture-pane",
                "-p",
                "-S",
                "-20",
                "-t",
                "=pessoal--x--claude-1:",
            ]))
            .await
            .unwrap();
        assert!(pane.contains("claude: not found"), "{pane}");
        // O servidor tmux do usuário não ganha a opção: só a janela da sessão.
        let global = t
            .tmux
            .run(&args(&["show-options", "-g", "-v", "remain-on-exit"]))
            .await
            .unwrap();
        assert_eq!(global.trim(), "off", "{global}");
        // A sessão morta não segura tarefa nem token.
        assert!(t.tmux.list_running().await.unwrap().is_empty());
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

    /// F14.3: a sessão morta continua no tmux, mas o token dela não vale mais.
    #[tokio::test]
    async fn exec_token_of_a_dead_session_is_refused() {
        let t = TestTmux::new("token-morto");
        let token = new_exec_token().unwrap();
        let info = t
            .tmux
            .create(
                &t.plan("pessoal--x--claude-1", "exit 3"),
                &spec("x", AgentKind::Claude),
                Some(&token),
            )
            .await
            .unwrap();
        assert_eq!(t.tmux.pane_state(&info.id).await, PaneState::Dead(Some(3)));
        assert_eq!(t.tmux.session_for_exec_token(&token).await.unwrap(), None);
    }

    /// F14.3: o painel vivo é `Alive`; o morto por sinal não tem código de saída.
    #[tokio::test]
    async fn pane_state_tells_alive_from_dead() {
        let t = TestTmux::new("estado");
        let info = t
            .tmux
            .create(
                &t.plan("pessoal--x--claude-1", "sleep 30"),
                &spec("x", AgentKind::Claude),
                None,
            )
            .await
            .unwrap();
        assert_eq!(t.tmux.pane_state(&info.id).await, PaneState::Alive);

        // O processo morre de sinal: não há código de saída (o tmux não inventa um).
        let info = t
            .tmux
            .create(
                &t.plan("pessoal--x--claude-2", "kill -9 $$"),
                &spec("x", AgentKind::Claude),
                None,
            )
            .await
            .unwrap();
        assert_eq!(t.tmux.pane_state(&info.id).await, PaneState::Dead(None));
    }

    /// F14.3: o status sai do painel mesmo quando o tmux o perde (o `SIGCHLD` que some).
    #[tokio::test]
    async fn pane_state_knows_the_status_of_a_dead_pane() {
        let t = TestTmux::new("status");
        let info = t
            .tmux
            .create(
                &t.plan("pessoal--x--claude-1", "exit 42"),
                &spec("x", AgentKind::Claude),
                None,
            )
            .await
            .unwrap();
        // O tmux 3.4 perde o SIGCHLD do painel morto em ~5% das vezes: o `pane_state` tem de
        // recuperar o status de saída mesmo assim (o `reap`).
        assert_eq!(t.tmux.pane_state(&info.id).await, PaneState::Dead(Some(42)));
    }

    #[tokio::test]
    async fn paste_puts_the_text_in_the_prompt_without_pressing_enter() {
        let t = TestTmux::new("colar");
        let out = t.dir.join("recebido.txt");
        let script = format!("cat > {}", out.display());
        let id = "pessoal--x--claude-1";
        t.tmux
            .create(&t.plan(id, &script), &spec("x", AgentKind::Claude), None)
            .await
            .unwrap();
        t.tmux.paste(id, "deixa verde").await.unwrap();
        // O texto entrou no prompt, mas o Enter é do usuário: o `cat` não recebeu nada.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let received = std::fs::read_to_string(&out).unwrap_or_default();
        assert!(received.is_empty(), "o texto rodou sozinho: {received}");
        // Com o Enter, o texto chega.
        t.tmux
            .run(&args(&["send-keys", "-t", id, "Enter"]))
            .await
            .unwrap();
        assert_eq!(wait_for_file(&out).await.trim(), "deixa verde");
    }

    #[tokio::test]
    async fn paste_needs_a_live_session_and_a_text() {
        let t = TestTmux::new("colar-erro");
        let err = t
            .tmux
            .paste("pessoal--x--claude-1", "oi")
            .await
            .unwrap_err();
        assert!(err.contains("não está viva"), "{err}");
        let id = "pessoal--x--claude-1";
        t.tmux
            .create(&t.plan(id, "sleep 30"), &spec("x", AgentKind::Claude), None)
            .await
            .unwrap();
        assert!(t.tmux.paste(id, "   ").await.is_err());
    }

    #[tokio::test]
    async fn paste_breaks_the_text_in_one_line() {
        let t = TestTmux::new("colar-linha");
        let out = t.dir.join("recebido.txt");
        let script = format!("cat > {}", out.display());
        let id = "pessoal--x--claude-1";
        t.tmux
            .create(&t.plan(id, &script), &spec("x", AgentKind::Claude), None)
            .await
            .unwrap();
        t.tmux.paste(id, "deixa\nverde").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let received = std::fs::read_to_string(&out).unwrap_or_default();
        assert!(
            received.is_empty(),
            "a quebra linha virou Enter: {received}"
        );
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
            dead: false,
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
        let listed = parse_line("pessoal--x--shell-1|~|pessoal|~|x|~|shell|~|10|~|1|~|0").unwrap();
        assert_eq!(listed.agent, AgentKind::Shell);
        assert_eq!(listed.attached, 1);
        assert!(!listed.dead);
        // Painel morto: a sessão continua listada, mas sem trabalho.
        let listed =
            parse_line("pessoal--x--claude-1|~|pessoal|~|x|~|claude|~|10|~|0|~|1").unwrap();
        assert!(listed.dead);
        assert!(parse_line("minha|~||~||~||~|10|~|0|~|0").is_none());
    }

    /// F14.4: a sessão do trabalhador não fica com o `remain-on-exit`: quando o processo
    /// termina bem, a sessão morre junto e não sobra aba morta na UI.
    #[tokio::test]
    async fn worker_session_dies_with_its_process() {
        let t = TestTmux::new("trabalhador");
        let id = "pessoal--x--opencode-1";
        let info = t
            .tmux
            .create(
                &t.plan_worker(id, "sleep 0.3; echo 'trabalhoi'; exit 0"),
                &spec("x", AgentKind::Opencode),
                None,
            )
            .await
            .unwrap();
        assert!(!info.dead, "a sessão começa viva");
        // O processo acabou: a sessão foi junto (sem `remain-on-exit`).
        for _ in 0..50 {
            if t.tmux.list().await.unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(
            t.tmux.list().await.unwrap().is_empty(),
            "a sessão do trabalhador que terminou bem ficou no tmux"
        );
    }

    /// F14.4: o trabalhador que desiste da tarefa pede o painel de volta: a sessão morre, mas
    /// o painel fica na tela com o erro.
    #[tokio::test]
    async fn keep_panel_leaves_the_dead_pane_on_screen() {
        let t = TestTmux::new("painel");
        let id = "pessoal--x--opencode-1";
        let info = t
            .tmux
            .create(
                &t.plan_worker(id, "sleep 0.5; echo 'falhou'; exit 1"),
                &spec("x", AgentKind::Opencode),
                None,
            )
            .await
            .unwrap();
        // O trabalhador desiste: o hubd liga o `remain-on-exit` da janela antes de ele sair.
        t.tmux.keep_panel(&info.id).await.unwrap();
        let mut state = PaneState::Alive;
        for _ in 0..50 {
            state = t.tmux.pane_state(&info.id).await;
            if state != PaneState::Alive {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(state, PaneState::Dead(Some(1)));
        let listed = t.tmux.list().await.unwrap();
        assert!(listed[0].dead, "a sessão fica no tmux, mas morta");
        // O erro continua visível no painel.
        let pane = t
            .tmux
            .run(&args(&[
                "capture-pane",
                "-p",
                "-S",
                "-20",
                "-t",
                "=pessoal--x--opencode-1:",
            ]))
            .await
            .unwrap();
        assert!(pane.contains("falhou"), "{pane}");
        // A sessão morta não segura tarefa nem token.
        assert!(t.tmux.list_running().await.unwrap().is_empty());
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
