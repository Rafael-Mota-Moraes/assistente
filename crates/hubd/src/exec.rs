//! Pedidos dos agentes para rodar um comando fora do ai-jail (F9.5). O agente pede pela API
//! dos agentes (`agent_listen`), você aprova no app e o hubd roda o comando **na VM**, na pasta
//! do projeto e com o env do perfil. A fila fica só em memória: reiniciar o hubd perde os
//! pedidos, e o cliente `fora-do-jail` avisa para pedir de novo.
//! Spec: docs/specs/05-sandbox.md ("Fora do jail")

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecState {
    Pending,
    Running,
    Done,
    Denied,
    Expired,
    Failed,
}

impl ExecState {
    pub fn is_final(self) -> bool {
        !matches!(self, Self::Pending | Self::Running)
    }
}

/// Onde o comando roda. `Host` (F9.9, pedido com `--pc`): o hubd nunca executa na VM; o app
/// roda no PC depois da aprovação, no mesmo caminho da pasta do projeto (virtiofs).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecTarget {
    #[default]
    Vm,
    Host,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExecResult {
    /// `None` quando o processo morreu por sinal (timeout) ou nem começou.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Saída maior que o limite: guardamos o fim, que é onde costuma estar o erro.
    pub truncated: bool,
    pub timed_out: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecView {
    pub id: String,
    /// Sessão (aba) que pediu.
    pub session: String,
    pub profile: String,
    pub project: String,
    pub cwd: PathBuf,
    pub command: String,
    /// `host` = pedido para o PC (F9.9): o hubd não executa, o app sim (F9.10).
    pub target: ExecTarget,
    pub timeout_secs: u64,
    pub created_at: u64,
    pub state: ExecState,
    /// Casou com um comando "sempre permitido" do projeto: rodou sem perguntar.
    pub auto_approved: bool,
    pub result: Option<ExecResult>,
    /// Pedido `host` já foi pego pelo app para execução no PC (F9.11).
    /// Evita que um segundo `host_exec` rode o comando de novo.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub picked_up: bool,
}

#[derive(Debug, Clone)]
pub struct ExecLimits {
    pub pending_ttl: Duration,
    pub keep_final: Duration,
    pub max_pending_per_session: usize,
    pub max_pending: usize,
    pub max_output: usize,
    pub default_timeout: Duration,
    pub max_timeout: Duration,
    /// Folga depois do timeout do pedido para o PC: o app tinha esse tempo para rodar
    /// e devolver o resultado (F9.10). Passou disso, o pedido expira.
    pub host_grace: Duration,
}

impl Default for ExecLimits {
    fn default() -> Self {
        Self {
            pending_ttl: Duration::from_secs(10 * 60),
            keep_final: Duration::from_secs(30 * 60),
            max_pending_per_session: 3,
            max_pending: 20,
            max_output: 64 * 1024,
            default_timeout: Duration::from_secs(10 * 60),
            max_timeout: Duration::from_secs(60 * 60),
            host_grace: Duration::from_secs(60),
        }
    }
}

/// O que é preciso para rodar um pedido aprovado.
#[derive(Debug, Clone)]
pub struct ExecJob {
    pub id: String,
    pub cwd: PathBuf,
    pub command: String,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
}

/// Pedido novo, já com sessão, cwd e env resolvidos pela rota.
pub struct NewRequest {
    pub session: String,
    pub profile: String,
    pub project: String,
    pub cwd: PathBuf,
    pub command: String,
    pub target: ExecTarget,
    pub env: Vec<(String, String)>,
    pub timeout_secs: Option<u64>,
    pub auto_approved: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExecError {
    NotFound,
    /// O pedido não está mais pendente (já aprovado, negado ou expirado).
    NotPending(ExecState),
    /// O pedido não é para o PC: só o app devolve resultado de pedido `host` (F9.10).
    NotHost,
    /// O app já pegou este pedido `host` (F9.11): um segundo `host_exec` não roda de novo.
    AlreadyClaimed,
    /// Resultado de pedido `host` que o app não pegou: só a falha antes de pegar entra.
    NotClaimed,
    TooMany,
}

struct Entry {
    view: ExecView,
    env: Vec<(String, String)>,
    created: Instant,
    /// Quando o pedido foi aprovado (o app no PC começa a rodar aqui, F9.10).
    approved: Option<Instant>,
    finished: Option<Instant>,
    state_tx: watch::Sender<ExecState>,
}

#[derive(Clone)]
pub struct ExecStore {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
    limits: Arc<ExecLimits>,
}

impl Default for ExecStore {
    fn default() -> Self {
        Self::new(ExecLimits::default())
    }
}

impl ExecStore {
    pub fn new(limits: ExecLimits) -> Self {
        Self {
            entries: Arc::default(),
            limits: Arc::new(limits),
        }
    }

    pub fn limits(&self) -> &ExecLimits {
        &self.limits
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Expira pendentes velhos e esquece resultados antigos. Roda em toda chamada.
    fn gc(entries: &mut HashMap<String, Entry>, limits: &ExecLimits) {
        let now = Instant::now();
        for entry in entries.values_mut() {
            let mut expire = false;
            if entry.view.state == ExecState::Pending
                && now.duration_since(entry.created) > limits.pending_ttl
            {
                expire = true;
            }
            // Pedido para o PC aprovado e sem resultado: o app pode ter morrido com o
            // pedido rodando (F9.10). Passou do timeout (mais a folga), expira.
            if entry.view.state == ExecState::Running && entry.view.target == ExecTarget::Host {
                let approved = entry.approved.unwrap_or(entry.created);
                let deadline = Duration::from_secs(entry.view.timeout_secs) + limits.host_grace;
                if now.duration_since(approved) > deadline {
                    expire = true;
                }
            }
            if expire {
                entry.view.state = ExecState::Expired;
                entry.finished = Some(now);
                entry.state_tx.send_replace(ExecState::Expired);
            }
        }
        entries.retain(|_, e| {
            e.finished
                .is_none_or(|t| now.duration_since(t) <= limits.keep_final)
        });
    }

    /// Registra o pedido. Auto-aprovado já sai como `Running`, junto com o job para rodar.
    pub fn submit(&self, request: NewRequest) -> Result<(ExecView, Option<ExecJob>), ExecError> {
        let mut entries = self.lock();
        Self::gc(&mut entries, &self.limits);
        let pending = |filter: &dyn Fn(&Entry) -> bool| {
            entries
                .values()
                .filter(|e| e.view.state == ExecState::Pending && filter(e))
                .count()
        };
        if !request.auto_approved
            && (pending(&|_| true) >= self.limits.max_pending
                || pending(&|e| e.view.session == request.session)
                    >= self.limits.max_pending_per_session)
        {
            return Err(ExecError::TooMany);
        }
        let timeout = request
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(self.limits.default_timeout)
            .clamp(Duration::from_secs(1), self.limits.max_timeout);
        let id = new_id();
        // Pedido para o PC (F9.9) nunca roda sozinho: sem "sempre permitir", sem job na VM.
        let auto_approved = request.auto_approved && request.target == ExecTarget::Vm;
        let state = if auto_approved {
            ExecState::Running
        } else {
            ExecState::Pending
        };
        let view = ExecView {
            id: id.clone(),
            session: request.session,
            profile: request.profile,
            project: request.project,
            cwd: request.cwd,
            command: request.command,
            target: request.target,
            timeout_secs: timeout.as_secs(),
            created_at: unix_now(),
            state,
            auto_approved,
            result: None,
            picked_up: false,
        };
        let job = auto_approved.then(|| job_of(&view, &request.env));
        let (state_tx, _) = watch::channel(state);
        entries.insert(
            id,
            Entry {
                view: view.clone(),
                env: request.env,
                created: Instant::now(),
                approved: None,
                finished: None,
                state_tx,
            },
        );
        Ok((view, job))
    }

    /// Pendentes primeiro, depois os mais recentes.
    pub fn list(&self) -> Vec<ExecView> {
        let mut entries = self.lock();
        Self::gc(&mut entries, &self.limits);
        let mut views: Vec<ExecView> = entries.values().map(|e| e.view.clone()).collect();
        views.sort_by(|a, b| {
            (b.state == ExecState::Pending)
                .cmp(&(a.state == ExecState::Pending))
                .then(b.created_at.cmp(&a.created_at))
                .then(b.id.cmp(&a.id))
        });
        views
    }

    pub fn get(&self, id: &str) -> Option<ExecView> {
        let mut entries = self.lock();
        Self::gc(&mut entries, &self.limits);
        entries.get(id).map(|e| e.view.clone())
    }

    /// Pendente → rodando; devolve o job para quem aprovou disparar. Pedido para o PC (F9.9)
    /// devolve `None`: o hubd não roda nada na VM, o app executa no host (F9.10).
    pub fn approve(&self, id: &str) -> Result<Option<ExecJob>, ExecError> {
        let mut entries = self.lock();
        Self::gc(&mut entries, &self.limits);
        let entry = entries.get_mut(id).ok_or(ExecError::NotFound)?;
        if entry.view.state != ExecState::Pending {
            return Err(ExecError::NotPending(entry.view.state));
        }
        entry.view.state = ExecState::Running;
        entry.approved = Some(Instant::now());
        entry.state_tx.send_replace(ExecState::Running);
        Ok((entry.view.target == ExecTarget::Vm).then(|| job_of(&entry.view, &entry.env)))
    }

    /// O app pega o pedido `host` aprovado para rodar no PC (F9.11): devolve o job uma vez
    /// só, e um segundo pedido do mesmo id dá `AlreadyClaimed` (não roda de novo).
    pub fn claim_host(&self, id: &str) -> Result<ExecJob, ExecError> {
        let mut entries = self.lock();
        Self::gc(&mut entries, &self.limits);
        let entry = entries.get_mut(id).ok_or(ExecError::NotFound)?;
        if entry.view.target != ExecTarget::Host {
            return Err(ExecError::NotHost);
        }
        if entry.view.state != ExecState::Running {
            return Err(ExecError::NotPending(entry.view.state));
        }
        if entry.view.picked_up {
            return Err(ExecError::AlreadyClaimed);
        }
        entry.view.picked_up = true;
        Ok(job_of(&entry.view, &entry.env))
    }

    pub fn deny(&self, id: &str) -> Result<ExecView, ExecError> {
        let mut entries = self.lock();
        Self::gc(&mut entries, &self.limits);
        let entry = entries.get_mut(id).ok_or(ExecError::NotFound)?;
        if entry.view.state != ExecState::Pending {
            return Err(ExecError::NotPending(entry.view.state));
        }
        entry.view.state = ExecState::Denied;
        entry.finished = Some(Instant::now());
        entry.state_tx.send_replace(ExecState::Denied);
        Ok(entry.view.clone())
    }

    /// Fim da execução: `Done` com o resultado (qualquer código de saída), `Failed` se nem
    /// conseguiu iniciar.
    pub fn finish(&self, id: &str, result: Result<ExecResult, String>) {
        let mut entries = self.lock();
        let Some(entry) = entries.get_mut(id) else {
            return;
        };
        Self::finish_entry(entry, result);
    }

    /// Resultado que o app no PC devolve (F9.10). Só vale para pedido `host` já aprovado
    /// (rodando): pedido da VM tem o resultado do hubd, e pendente ainda não foi aprovado.
    /// Saída só de pedido que o app pegou (F9.11); a falha antes de pegar também fecha o
    /// pedido, para o agente não ficar esperando.
    pub fn finish_host(
        &self,
        id: &str,
        result: Result<ExecResult, String>,
    ) -> Result<(), ExecError> {
        let mut entries = self.lock();
        Self::gc(&mut entries, &self.limits);
        let entry = entries.get_mut(id).ok_or(ExecError::NotFound)?;
        if entry.view.target != ExecTarget::Host {
            return Err(ExecError::NotHost);
        }
        if entry.view.state != ExecState::Running {
            return Err(ExecError::NotPending(entry.view.state));
        }
        if !entry.view.picked_up && result.is_ok() {
            return Err(ExecError::NotClaimed);
        }
        Self::finish_entry(entry, result);
        Ok(())
    }

    /// Grava o resultado e fecha o pedido: `Done` (com o resultado) ou `Failed` (nem
    /// conseguiu rodar).
    fn finish_entry(entry: &mut Entry, result: Result<ExecResult, String>) {
        let state = match result {
            Ok(result) => {
                entry.view.result = Some(result);
                ExecState::Done
            }
            Err(message) => {
                entry.view.result = Some(ExecResult {
                    stderr: message,
                    ..ExecResult::default()
                });
                ExecState::Failed
            }
        };
        entry.view.state = state;
        entry.finished = Some(Instant::now());
        entry.state_tx.send_replace(state);
    }

    /// Espera até `wait` por um estado final (long-poll do cliente). `None` = não existe.
    pub async fn wait(&self, id: &str, wait: Duration) -> Option<ExecView> {
        let (mut rx, expires_in) = {
            let mut entries = self.lock();
            Self::gc(&mut entries, &self.limits);
            let entry = entries.get(id)?;
            // Pendente que expira no meio da espera: o gc só roda em chamadas, então acorda a
            // tempo de expirar. Pedido do PC aprovado sem resultado também tem prazo (F9.10).
            let expires_in = match entry.view.state {
                ExecState::Pending => Some(
                    self.limits
                        .pending_ttl
                        .saturating_sub(entry.created.elapsed())
                        + Duration::from_millis(50),
                ),
                ExecState::Running if entry.view.target == ExecTarget::Host => {
                    let approved = entry.approved.unwrap_or(entry.created);
                    let deadline =
                        Duration::from_secs(entry.view.timeout_secs) + self.limits.host_grace;
                    Some(deadline.saturating_sub(approved.elapsed()) + Duration::from_millis(50))
                }
                _ => None,
            };
            (entry.state_tx.subscribe(), expires_in)
        };
        let wait = expires_in.map_or(wait, |left| wait.min(left));
        let _ = tokio::time::timeout(wait, rx.wait_for(|s| s.is_final())).await;
        self.get(id)
    }
}

fn job_of(view: &ExecView, env: &[(String, String)]) -> ExecJob {
    ExecJob {
        id: view.id.clone(),
        cwd: view.cwd.clone(),
        command: view.command.clone(),
        env: env.to_vec(),
        timeout: Duration::from_secs(view.timeout_secs),
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Id curto e imprevisível (o cliente só lê o resultado do próprio pedido de qualquer jeito).
fn new_id() -> String {
    crate::sessions::new_exec_token()
        .map(|t| t[..16].to_owned())
        .unwrap_or_else(|_| format!("{:x}", unix_now()))
}

/// Roda `bash -lc <comando>` fora do jail, num grupo de processos próprio: no timeout o
/// grupo inteiro morre (inclusive o que o comando deixou em segundo plano).
pub async fn run_command(job: &ExecJob, max_output: usize) -> Result<ExecResult, String> {
    let started = Instant::now();
    let mut command = tokio::process::Command::new("bash");
    command
        .arg("-lc")
        .arg(&job.command)
        .current_dir(&job.cwd)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    // O mínimo do hubd (systemd) + o env do perfil, como na aba Terminal.
    for key in ["HOME", "USER", "LOGNAME", "PATH", "SHELL"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.envs(job.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    command.env("GIT_TERMINAL_PROMPT", "0");
    let mut child = command
        .spawn()
        .map_err(|e| format!("não consegui iniciar o bash: {e}"))?;
    let pid = child.id();
    let stdout = tokio::spawn(read_tail(child.stdout.take(), max_output));
    let stderr = tokio::spawn(read_tail(child.stderr.take(), max_output));

    let (exit_code, timed_out) = match tokio::time::timeout(job.timeout, child.wait()).await {
        Ok(status) => (
            status
                .map_err(|e| format!("erro esperando o comando: {e}"))?
                .code(),
            false,
        ),
        Err(_) => {
            if let Some(pid) = pid {
                kill_group(pid).await;
            }
            let _ = child.wait().await;
            (None, true)
        }
    };
    // Um neto que fechou o grupo (setsid) pode segurar o pipe: não espera para sempre.
    let collect = |task: tokio::task::JoinHandle<(String, bool)>| async move {
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
    };
    let (stdout, cut_out) = collect(stdout).await;
    let (stderr, cut_err) = collect(stderr).await;
    Ok(ExecResult {
        exit_code,
        stdout,
        stderr,
        truncated: cut_out || cut_err,
        timed_out,
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

async fn kill_group(pid: u32) {
    let group = format!("-{pid}");
    for (signal, grace) in [("-TERM", Duration::from_secs(2)), ("-KILL", Duration::ZERO)] {
        let _ = tokio::process::Command::new("kill")
            .args([signal, "--", &group])
            .stderr(Stdio::null())
            .status()
            .await;
        tokio::time::sleep(grace).await;
    }
}

/// Lê tudo e guarda só os últimos `max` bytes (em UTF-8 válido).
async fn read_tail(stream: Option<impl AsyncRead + Unpin>, max: usize) -> (String, bool) {
    let Some(mut stream) = stream else {
        return (String::new(), false);
    };
    let mut kept: Vec<u8> = Vec::new();
    let mut truncated = false;
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                kept.extend_from_slice(&buf[..n]);
                if kept.len() > max {
                    kept.drain(..kept.len() - max);
                    truncated = true;
                }
            }
        }
    }
    (String::from_utf8_lossy(&kept).into_owned(), truncated)
}

/// `cwd` pedido pelo agente (absoluto) precisa ficar dentro do projeto; vazio = a raiz dele.
pub fn resolve_cwd(project_dir: &Path, requested: Option<&str>) -> Result<PathBuf, String> {
    let root = project_dir
        .canonicalize()
        .map_err(|e| format!("{}: {e}", project_dir.display()))?;
    let Some(requested) = requested.filter(|r| !r.is_empty()) else {
        return Ok(root);
    };
    let path = Path::new(requested)
        .canonicalize()
        .map_err(|e| format!("cwd {requested}: {e}"))?;
    if path.starts_with(&root) {
        Ok(path)
    } else {
        Err(format!("cwd {requested} fica fora do projeto"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(session: &str, command: &str) -> NewRequest {
        NewRequest {
            session: session.into(),
            profile: "pessoal".into(),
            project: "x".into(),
            cwd: std::env::temp_dir(),
            command: command.into(),
            target: ExecTarget::Vm,
            env: vec![("ASSISTENTE_TESTE".into(), "sim".into())],
            timeout_secs: None,
            auto_approved: false,
        }
    }

    fn job(command: &str, timeout: Duration) -> ExecJob {
        ExecJob {
            id: "t".into(),
            cwd: std::env::temp_dir(),
            command: command.into(),
            env: vec![("ASSISTENTE_TESTE".into(), "sim".into())],
            timeout,
        }
    }

    #[tokio::test]
    async fn run_command_returns_code_output_cwd_and_env() {
        let result = run_command(
            &job(
                "echo \"$ASSISTENTE_TESTE $PWD\"; echo erro >&2; exit 3",
                Duration::from_secs(10),
            ),
            1024,
        )
        .await
        .unwrap();
        assert_eq!(result.exit_code, Some(3));
        let tmp = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(result.stdout.trim(), format!("sim {}", tmp.display()));
        assert_eq!(result.stderr.trim(), "erro");
        assert!(!result.timed_out && !result.truncated);
    }

    #[tokio::test]
    async fn timeout_kills_the_whole_group() {
        let marker = std::env::temp_dir().join(format!("hubd-exec-grupo-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        // O filho em segundo plano criaria o arquivo depois do timeout se sobrevivesse.
        let script = format!("(sleep 3; touch {}) & sleep 30", marker.display());
        let started = Instant::now();
        let result = run_command(&job(&script, Duration::from_millis(300)), 1024)
            .await
            .unwrap();
        assert!(result.timed_out);
        assert_eq!(result.exit_code, None);
        assert!(started.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_secs(4)).await;
        assert!(!marker.exists(), "o processo em segundo plano sobreviveu");
    }

    #[tokio::test]
    async fn long_output_keeps_the_tail() {
        let result = run_command(
            &job(
                "for i in $(seq 1 2000); do echo linha-$i; done",
                Duration::from_secs(10),
            ),
            100,
        )
        .await
        .unwrap();
        assert!(result.truncated);
        assert!(result.stdout.len() <= 100);
        assert!(result.stdout.trim_end().ends_with("linha-2000"));
    }

    #[tokio::test]
    async fn approve_deny_and_wait() {
        let store = ExecStore::default();
        let (view, job) = store.submit(request("s1", "echo oi")).unwrap();
        assert_eq!(view.state, ExecState::Pending);
        assert!(job.is_none());
        assert_eq!(store.list()[0].id, view.id);

        let waited = store
            .wait(&view.id, Duration::from_millis(50))
            .await
            .unwrap();
        assert_eq!(waited.state, ExecState::Pending);

        let job = store.approve(&view.id).unwrap().unwrap();
        assert_eq!(job.env, [("ASSISTENTE_TESTE".into(), "sim".into())]);
        assert_eq!(
            store.approve(&view.id).unwrap_err(),
            ExecError::NotPending(ExecState::Running)
        );
        let id = view.id.clone();
        let waiter = {
            let store = store.clone();
            tokio::spawn(async move { store.wait(&id, Duration::from_secs(10)).await })
        };
        let result = run_command(&job, 1024).await;
        store.finish(&view.id, result);
        let done = waiter.await.unwrap().unwrap();
        assert_eq!(done.state, ExecState::Done);
        assert_eq!(done.result.unwrap().stdout, "oi\n");
        // Quem chega depois (sem ninguém ouvindo na hora do fim) vê o estado final na hora.
        let started = Instant::now();
        let late = store.wait(&view.id, Duration::from_secs(10)).await.unwrap();
        assert_eq!(late.state, ExecState::Done);
        assert!(started.elapsed() < Duration::from_secs(1));

        let (other, _) = store.submit(request("s1", "rm -rf /")).unwrap();
        assert_eq!(store.deny(&other.id).unwrap().state, ExecState::Denied);
        let started = Instant::now();
        let denied = store
            .wait(&other.id, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(denied.state, ExecState::Denied);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "negado sem ouvinte travou a espera"
        );
        assert_eq!(store.deny("nao-existe").unwrap_err(), ExecError::NotFound);
    }

    #[tokio::test]
    async fn limits_expire_and_forget() {
        let store = ExecStore::new(ExecLimits {
            pending_ttl: Duration::from_millis(100),
            keep_final: Duration::from_millis(300),
            max_pending_per_session: 2,
            max_pending: 3,
            ..ExecLimits::default()
        });
        store.submit(request("s1", "a")).unwrap();
        let (old, _) = store.submit(request("s1", "b")).unwrap();
        assert_eq!(
            store.submit(request("s1", "c")).unwrap_err(),
            ExecError::TooMany
        );
        store.submit(request("s2", "d")).unwrap();
        assert_eq!(
            store.submit(request("s3", "e")).unwrap_err(),
            ExecError::TooMany
        );
        // Auto-aprovado não conta como pendente.
        let auto = NewRequest {
            auto_approved: true,
            ..request("s1", "cargo build")
        };
        let (view, job) = store.submit(auto).unwrap();
        assert_eq!(view.state, ExecState::Running);
        assert!(job.is_some());

        // Expira no meio do long-poll.
        let started = Instant::now();
        let waited = store.wait(&old.id, Duration::from_secs(10)).await.unwrap();
        assert_eq!(waited.state, ExecState::Expired);
        assert!(started.elapsed() < Duration::from_secs(2));
        store.submit(request("s1", "f")).unwrap();

        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(store.get(&old.id).is_none(), "expirado antigo é esquecido");
    }

    #[test]
    fn cwd_must_stay_inside_the_project() {
        let dir = std::env::temp_dir().join(format!("hubd-exec-cwd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("proj/sub")).unwrap();
        let project = dir.join("proj");
        let root = project.canonicalize().unwrap();
        assert_eq!(resolve_cwd(&project, None).unwrap(), root);
        let sub = project.join("sub").display().to_string();
        assert_eq!(resolve_cwd(&project, Some(&sub)).unwrap(), root.join("sub"));
        let escape = project.join("sub/../..").display().to_string();
        assert!(resolve_cwd(&project, Some(&escape)).is_err());
        assert!(resolve_cwd(&project, Some("/etc")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Pedido com `--pc` (F9.9): aparece com o alvo, nunca é auto-aprovado e o hubd não gera
    /// job para a VM — quem executa é o app no PC (F9.10). Sem resposta, expira como os outros.
    #[tokio::test]
    async fn host_requests_wait_for_the_app_and_never_run_on_the_vm() {
        let store = ExecStore::new(ExecLimits {
            pending_ttl: Duration::from_millis(100),
            ..ExecLimits::default()
        });
        let (view, job) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                auto_approved: true,
                ..request("s1", "git push")
            })
            .unwrap();
        assert_eq!(view.target, ExecTarget::Host);
        assert_eq!(view.state, ExecState::Pending);
        assert!(!view.auto_approved, "pedido para o PC nunca roda sozinho");
        assert!(job.is_none(), "o hubd não gera job de VM para o PC");
        // Aprovar só marca como rodando: o app executa no PC e devolve o resultado (F9.10).
        assert!(store.approve(&view.id).unwrap().is_none());
        assert_eq!(store.get(&view.id).unwrap().state, ExecState::Running);

        // Um pendente sem resposta expira como qualquer outro.
        let (waiting, _) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                ..request("s1", "ls")
            })
            .unwrap();
        let waited = store
            .wait(&waiting.id, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(waited.state, ExecState::Expired);
    }

    /// F9.10: o resultado do app só entra em pedido `host` já aprovado. Pedido da VM é
    /// recusado, pendente também, e o aprovado sem resultado (app morreu) expira.
    #[tokio::test]
    async fn host_result_is_only_accepted_for_an_approved_host_request() {
        let store = ExecStore::new(ExecLimits {
            host_grace: Duration::from_millis(50),
            ..ExecLimits::default()
        });
        let (host, _) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                ..request("s1", "git push")
            })
            .unwrap();
        // Ainda pendente: ninguém aprovou no app.
        assert_eq!(
            store
                .finish_host(&host.id, Ok(ExecResult::default()))
                .unwrap_err(),
            ExecError::NotPending(ExecState::Pending)
        );
        store.approve(&host.id).unwrap();
        // Pedido da VM: o resultado vem do hubd, não do app.
        let (vm, _) = store.submit(request("s1", "ls")).unwrap();
        assert_eq!(
            store
                .finish_host(&vm.id, Ok(ExecResult::default()))
                .unwrap_err(),
            ExecError::NotHost
        );
        // Aprovado e pego pelo app (F9.11): o resultado do app entra.
        store.claim_host(&host.id).unwrap();
        let result = ExecResult {
            exit_code: Some(0),
            stdout: "ok\n".into(),
            ..ExecResult::default()
        };
        store.finish_host(&host.id, Ok(result)).unwrap();
        let done = store.get(&host.id).unwrap();
        assert_eq!(done.state, ExecState::Done);
        assert_eq!(done.result.unwrap().stdout, "ok\n");

        // O app não conseguiu rodar: o agente recebe o erro.
        let (broken, _) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                ..request("s1", "git push")
            })
            .unwrap();
        store.approve(&broken.id).unwrap();
        store
            .finish_host(&broken.id, Err("não consegui iniciar o bash".into()))
            .unwrap();
        let failed = store.get(&broken.id).unwrap();
        assert_eq!(failed.state, ExecState::Failed);
        assert_eq!(failed.result.unwrap().stderr, "não consegui iniciar o bash");

        // Aprovado e sem resultado (o app morreu): expira, o agente não espera para sempre.
        let (lost, _) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                timeout_secs: Some(1),
                ..request("s1", "git push")
            })
            .unwrap();
        store.approve(&lost.id).unwrap();
        tokio::time::sleep(Duration::from_millis(1200)).await;
        let expired = store.wait(&lost.id, Duration::from_secs(5)).await.unwrap();
        assert_eq!(expired.state, ExecState::Expired);
    }

    /// F9.11: claim_host marca o pedido como pego e devolve o job; segundo claim falha.
    #[tokio::test]
    async fn claim_host_marks_picked_up_and_prevents_double_execution() {
        let store = ExecStore::default();
        let (host, _) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                ..request("s1", "git push")
            })
            .unwrap();
        store.approve(&host.id).unwrap();

        // Primeiro claim: deve funcionar e devolver o job.
        let job = store.claim_host(&host.id).unwrap();
        assert_eq!(job.command, "git push");
        assert!(!job.id.is_empty());

        // O view deve mostrar picked_up = true.
        let view = store.get(&host.id).unwrap();
        assert!(view.picked_up);
        assert_eq!(view.state, ExecState::Running);

        // Segundo claim: deve falhar (já foi pego).
        let err = store.claim_host(&host.id).unwrap_err();
        assert_eq!(err, ExecError::AlreadyClaimed);

        // finish_host deve funcionar após claim.
        store
            .finish_host(&host.id, Ok(ExecResult::default()))
            .unwrap();
        let done = store.get(&host.id).unwrap();
        assert_eq!(done.state, ExecState::Done);
    }

    /// F9.11: claim_host falha se o pedido não for host, não estiver aprovado, ou não existir.
    #[tokio::test]
    async fn claim_host_rejects_invalid_requests() {
        let store = ExecStore::default();

        // Pedido VM: não pode fazer claim.
        let (vm, _) = store.submit(request("s1", "ls")).unwrap();
        store.approve(&vm.id).unwrap();
        assert_eq!(store.claim_host(&vm.id).unwrap_err(), ExecError::NotHost);

        // Pedido host pendente (não aprovado): não pode fazer claim.
        let (pending, _) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                ..request("s1", "git push")
            })
            .unwrap();
        assert_eq!(
            store.claim_host(&pending.id).unwrap_err(),
            ExecError::NotPending(ExecState::Pending)
        );

        // ID inexistente: NotFound.
        assert_eq!(
            store.claim_host("nao-existe").unwrap_err(),
            ExecError::NotFound
        );
    }

    /// F9.11: saída só de pedido pego; a falha antes de pegar fecha o pedido.
    #[tokio::test]
    async fn finish_host_requires_claim() {
        let store = ExecStore::default();
        let (host, _) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                ..request("s1", "git push")
            })
            .unwrap();
        store.approve(&host.id).unwrap();

        // Sem claim, finish_host deve falhar.
        assert_eq!(
            store
                .finish_host(&host.id, Ok(ExecResult::default()))
                .unwrap_err(),
            ExecError::NotClaimed
        );

        // Com claim, finish_host deve funcionar.
        store.claim_host(&host.id).unwrap();
        store
            .finish_host(&host.id, Ok(ExecResult::default()))
            .unwrap();
        let done = store.get(&host.id).unwrap();
        assert_eq!(done.state, ExecState::Done);

        // O app falhou antes de pegar (config, hubd fora): o erro fecha o pedido.
        let (lost, _) = store
            .submit(NewRequest {
                target: ExecTarget::Host,
                ..request("s1", "git push")
            })
            .unwrap();
        store.approve(&lost.id).unwrap();
        store
            .finish_host(&lost.id, Err("sem config".into()))
            .unwrap();
        assert_eq!(store.get(&lost.id).unwrap().state, ExecState::Failed);
    }
}
