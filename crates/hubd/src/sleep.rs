//! Orquestração do modo dormir no hubd (F6.2–F6.4): levar o HEAD do PC até o clone no
//! notebook, gerar o prompt com o briefing do ai-memory, iniciar o `dorme` num tmux de lá,
//! acompanhar (status + log), parar (STOP) e recolher de manhã (fetch + merge).
//! Spec: docs/specs/08-modo-dormir.md.
//!
//! O hubd roda na VM, onde as raízes dos perfis estão montadas no MESMO caminho do PC
//! (virtiofs), então `git -C <raiz>/<perfil>/<projeto>` aqui vê o repo do PC. O notebook é
//! alcançado pelo alias ssh da config (`Remote::Ssh`); nos testes, `Remote::Local` roda o
//! mesmo script bash num diretório local (dois repos locais simulando PC e notebook, e o
//! tmux do devenv no lugar do tmux do notebook).

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Onde fica o "notebook": alias ssh (produção) ou o próprio host (testes).
#[derive(Debug, Clone)]
pub enum Remote {
    Ssh {
        host: String,
    },
    /// O "notebook" é o próprio host: usado nos testes do devenv (tmux e git locais).
    #[cfg_attr(not(test), allow(dead_code))]
    Local,
}

impl Remote {
    /// Roda um script bash no remoto e devolve o stdout; falha com o stderr na mensagem.
    pub(crate) async fn shell_out(&self, script: &str) -> Result<String> {
        let output = match self {
            // `bash -s` lê o script do stdin: sem camada extra de quoting no ssh.
            Remote::Ssh { host } => {
                let mut child = Command::new("ssh")
                    .args([host.as_str(), "bash -s"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .with_context(|| format!("ssh {host}"))?;
                let mut stdin = child.stdin.take().expect("stdin pipe");
                stdin.write_all(script.as_bytes()).await?;
                drop(stdin);
                child.wait_with_output().await?
            }
            Remote::Local => {
                Command::new("bash")
                    .args(["-e", "-c", script])
                    .output()
                    .await?
            }
        };
        if !output.status.success() {
            bail!(
                "comando remoto falhou ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Como `shell_out`, sem aproveitar o stdout.
    async fn shell(&self, script: &str) -> Result<()> {
        self.shell_out(script).await?;
        Ok(())
    }

    /// Alvo do `git push`: `host:caminho` (ssh, com `~` expandido pelo shell remoto) ou o
    /// caminho local.
    fn push_target(&self, remote_dir: &str) -> String {
        match self {
            Remote::Ssh { host } => format!("{host}:{remote_dir}"),
            Remote::Local => remote_dir.to_owned(),
        }
    }
}

/// O que o sync deixou pronto (a rota F6.3 devolve para a UI/log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Branch do PC que estava no HEAD (a "base" do modo dormir).
    pub base: String,
    /// Commit do PC que o notebook passou a ter.
    pub head: String,
    /// Caminho do clone no notebook.
    pub remote_dir: String,
    /// Branch de trabalho criada no notebook (`dorme/<data>`).
    pub sleep_branch: String,
}

/// `~/x` vira `$HOME/x` para o script bash remoto (aspas duplas expandem; `~` entre aspas
/// não).
pub(crate) fn shell_dir(remote_dir: &str) -> String {
    match remote_dir.strip_prefix("~/") {
        Some(rest) => format!("$HOME/{rest}"),
        None if remote_dir == "~" => "$HOME".to_owned(),
        None => remote_dir.to_owned(),
    }
}

/// Branch, data e caminho entram num script shell e num ref do git: nada de metacaracteres.
pub(crate) fn validate(part: &str, what: &str) -> Result<()> {
    let bad = part.is_empty()
        || part.contains("..")
        || part.chars().any(|c| {
            matches!(
                c,
                '"' | '\'' | '`' | '$' | '\\' | ' ' | '\n' | ';' | '&' | '|'
            )
        });
    if bad {
        bail!("{what} suspeito: {part:?}");
    }
    Ok(())
}

async fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .await
        .with_context(|| format!("git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} falhou: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Passos 2 e 3 da spec 08: cria o clone no notebook na primeira vez (`git init`), faz o
/// push do HEAD do PC para `refs/remotes/pc/<base>` (com force: o notebook fica com
/// exatamente o HEAD do PC) e cria/reseta a branch `dorme/<data>` a partir dele.
///
/// Branches e commits que já são do notebook não são tocados: só escrevemos em
/// `refs/remotes/pc/*` e em `dorme/<data>`.
pub async fn sync_project(
    project_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
    date: &str,
) -> Result<SyncReport> {
    validate(remote_dir, "caminho remoto")?;
    validate(date, "data")?;
    let base = git(project_dir, &["branch", "--show-current"]).await?;
    if base.is_empty() {
        bail!("o HEAD do PC não está numa branch (detached?); faça checkout antes de dormir");
    }
    validate(&base, "branch base")?;
    let head = git(project_dir, &["rev-parse", "HEAD"]).await?;

    let dir = shell_dir(remote_dir);
    // Clone criado na primeira vez; repo existente é reutilizado. A cada sync, o `.ai-jail`
    // que o ai-jail cria no clone sai do `git status` (senão o `dorme` para na 1ª tarefa).
    remote
        .shell(&format!(
            "mkdir -p \"{dir}\"\n\
             git -C \"{dir}\" rev-parse --git-dir >/dev/null 2>&1 || git -C \"{dir}\" init -q -b main\n\
             cd \"{dir}\"\n\
             f=\"$(git rev-parse --git-path info/exclude)\"\n\
             mkdir -p \"$(dirname \"$f\")\"\n\
             grep -qxF '{JAIL_CONFIG_EXCLUDE}' \"$f\" 2>/dev/null || echo '{JAIL_CONFIG_EXCLUDE}' >> \"$f\"\n"
        ))
        .await
        .context("preparando o clone no notebook")?;

    let target = remote.push_target(remote_dir);
    git(
        project_dir,
        &[
            "push",
            "--force",
            &target,
            &format!("HEAD:refs/remotes/pc/{base}"),
        ],
    )
    .await
    .context("push do HEAD do PC para o notebook")?;

    let sleep_branch = format!("dorme/{date}");
    remote
        .shell(&format!(
            "git -C \"{dir}\" switch -C \"{sleep_branch}\" \"pc/{base}\"\n"
        ))
        .await
        .context("criando a branch de trabalho no notebook")?;

    Ok(SyncReport {
        base,
        head,
        remote_dir: remote_dir.to_owned(),
        sleep_branch,
    })
}

/// Um commit que veio do notebook (F6.5).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CollectedCommit {
    pub hash: String,
    pub subject: String,
}

/// O que a manhã encontrou no notebook (F6.5): o que o agente fez, se há relatório e se a
/// base do PC aceita fast-forward.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CollectReport {
    /// Branch do PC que o merge alcançaria.
    pub base: String,
    /// Branch de trabalho que veio do notebook (`dorme/<data>`).
    pub sleep_branch: String,
    /// Commits first-parent de `base..notebook/dorme/<data>`, do mais antigo ao mais novo.
    pub commits: Vec<CollectedCommit>,
    /// Conteúdo do RELATORIO.md do notebook (ele não é commitado; lido via shell).
    pub relatorio: Option<String>,
    /// `true` = a base é ancestral da branch recolhida: merge com um clique.
    pub fast_forward: bool,
}

/// Passo 8 da spec 08: fetch de `dorme/<data>` e `agente/*` do notebook para
/// `refs/remotes/notebook/*` no repo do PC e resumo do que chegou.
pub async fn collect_project(
    project_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
    date: &str,
) -> Result<CollectReport> {
    validate(remote_dir, "caminho remoto")?;
    validate(date, "data")?;
    let base = git(project_dir, &["branch", "--show-current"]).await?;
    if base.is_empty() {
        bail!("o HEAD do PC não está numa branch (detached?); faça checkout antes de recolher");
    }
    validate(&base, "branch base")?;
    let target = remote.push_target(remote_dir);
    let sleep_ref = format!("refs/remotes/notebook/dorme/{date}");

    // Sem a branch de trabalho não houve dormir nessa data (ou o clone sumiu).
    git(
        project_dir,
        &[
            "fetch",
            &target,
            &format!("+refs/heads/dorme/{date}:{sleep_ref}"),
        ],
    )
    .await
    .with_context(|| format!("buscando dorme/{date} no notebook"))?;
    // Branches de WIP: curinga sem correspondência não é erro; falha real só avisa.
    if let Err(e) = git(
        project_dir,
        &[
            "fetch",
            &target,
            "+refs/heads/agente/*:refs/remotes/notebook/agente/*",
        ],
    )
    .await
    {
        tracing::warn!("agente/* do notebook não veio: {e}");
    }

    let log = git(
        project_dir,
        &[
            "log",
            "--reverse",
            "--first-parent",
            "--format=%H%x1f%s",
            &format!("HEAD..{sleep_ref}"),
        ],
    )
    .await?;
    let commits = log
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (hash, subject) = line.split_once('\u{1f}').unwrap_or((line, ""));
            CollectedCommit {
                hash: hash.to_owned(),
                subject: subject.to_owned(),
            }
        })
        .collect();

    // O agente escreve RELATORIO.md sem commitar (contrato da spec 08): lê na árvore.
    let dir = shell_dir(remote_dir);
    let relatorio = remote
        .shell_out(&format!(
            "if [ -f \"{dir}/RELATORIO.md\" ]; then cat \"{dir}/RELATORIO.md\"; fi"
        ))
        .await
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());

    let fast_forward = Command::new("git")
        .current_dir(project_dir)
        .args(["merge-base", "--is-ancestor", "HEAD", &sleep_ref])
        .output()
        .await
        .is_ok_and(|output| output.status.success());

    Ok(CollectReport {
        base,
        sleep_branch: format!("dorme/{date}"),
        commits,
        relatorio,
        fast_forward,
    })
}

/// Passo 9 da spec 08 (o clique na UI): merge fast-forward da branch recolhida na base
/// atual. Se a base andou durante a noite, não dá: nada muda e a UI avisa para mesclar
/// à mão.
pub async fn merge_sleep_branch(project_dir: &Path, date: &str) -> Result<()> {
    validate(date, "data")?;
    git(
        project_dir,
        &[
            "merge",
            "--ff-only",
            &format!("refs/remotes/notebook/dorme/{date}"),
        ],
    )
    .await
    .context("merge fast-forward (recolha antes com collect)")?;
    Ok(())
}

// ---- Sync da memória (wiki) PC ↔ notebook (F5.4) ----
//
// O espelho em `memory.mirror_dir` (na verdade `mirror_dir/wiki`, o repo git do wiki do
// ai-memory) é a fonte. Antes/depois do dormir ele é empurrado/buscado do notebook, igual
// ao sync de projeto (F6.2), mas sem branch de trabalho: o notebook fica com exatamente o
// HEAD do PC e, de volta, traz os commits que o agente noturno fez no wiki.

/// Onde o wiki git mora de fato: `mirror_dir` se ele mesmo é um repo, senão `mirror_dir/wiki`.
pub fn wiki_repo(mirror_dir: &Path) -> PathBuf {
    if mirror_dir.join(".git").exists() {
        mirror_dir.to_path_buf()
    } else {
        mirror_dir.join("wiki")
    }
}

async fn ensure_wiki_repo(wiki: &Path) -> Result<()> {
    if !wiki.join(".git").exists() {
        if let Some(parent) = wiki.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::create_dir_all(wiki).ok();
        git(wiki, &["init", "-q", "-b", "main"]).await?;
        let _ = git(wiki, &["config", "user.email", "assistente@local"]).await;
        let _ = git(wiki, &["config", "user.name", "Assistente"]).await;
    }
    // Repo vazio não tem HEAD: cria um commit inicial vazio para o push não falhar.
    if git(wiki, &["rev-parse", "HEAD"]).await.is_err() {
        git(wiki, &["commit", "-q", "--allow-empty", "-m", "init"]).await?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MemorySyncReport {
    pub head: String,
    pub remote_dir: String,
}

/// Empurra o wiki do PC para o clone do notebook (cria o clone na primeira vez). Depois
/// disso o notebook tem exatamente o HEAD do PC.
pub async fn sync_memory(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemorySyncReport> {
    validate(remote_dir, "caminho remoto")?;
    let wiki = wiki_repo(mirror_dir);
    ensure_wiki_repo(&wiki).await?;
    let head = git(&wiki, &["rev-parse", "HEAD"]).await?;

    let dir = shell_dir(remote_dir);
    remote
        .shell(&format!(
            "mkdir -p \"{dir}\"\n\
             git -C \"{dir}\" rev-parse --git-dir >/dev/null 2>&1 || git -C \"{dir}\" init -q -b main\n"
        ))
        .await
        .context("preparando o clone da memória no notebook")?;

    let target = remote.push_target(remote_dir);
    git(
        &wiki,
        &["push", "--force", &target, "HEAD:refs/remotes/pc/main"],
    )
    .await
    .context("push da memória (wiki) para o notebook")?;

    let _ = remote
        .shell(&format!("git -C \"{dir}\" switch -C main pc/main\n"))
        .await;

    Ok(MemorySyncReport {
        head,
        remote_dir: remote_dir.to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MemoryCollectReport {
    pub fast_forward: bool,
    pub remote_dir: String,
}

/// Busca o wiki do notebook para `refs/remotes/notebook/memory` no PC e diz se a base aceita
/// fast-forward (para a manhã trazer as notas feitas à noite).
pub async fn collect_memory(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemoryCollectReport> {
    validate(remote_dir, "caminho remoto")?;
    let wiki = wiki_repo(mirror_dir);
    ensure_wiki_repo(&wiki).await?;
    let target = remote.push_target(remote_dir);
    git(
        &wiki,
        &[
            "fetch",
            &target,
            "+refs/heads/main:refs/remotes/notebook/memory",
        ],
    )
    .await
    .context("buscando a memória (wiki) do notebook")?;

    let fast_forward = Command::new("git")
        .current_dir(&wiki)
        .args([
            "merge-base",
            "--is-ancestor",
            "HEAD",
            "refs/remotes/notebook/memory",
        ])
        .output()
        .await
        .is_ok_and(|output| output.status.success());

    Ok(MemoryCollectReport {
        fast_forward,
        remote_dir: remote_dir.to_owned(),
    })
}

/// Merge fast-forward da memória recolhida (só se a base for ancestral).
#[allow(dead_code)]
pub async fn merge_memory(mirror_dir: &Path) -> Result<()> {
    let wiki = wiki_repo(mirror_dir);
    git(
        &wiki,
        &["merge", "--ff-only", "refs/remotes/notebook/memory"],
    )
    .await
    .context("merge fast-forward da memória (recolha antes com collect_memory)")?;
    Ok(())
}

// Aliases para nomes prováveis usados por testes/docs.
#[allow(dead_code)]
pub async fn sync_wiki(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemorySyncReport> {
    sync_memory(mirror_dir, remote, remote_dir).await
}
#[allow(dead_code)]
pub async fn sync_mirror(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemorySyncReport> {
    sync_memory(mirror_dir, remote, remote_dir).await
}
#[allow(dead_code)]
pub async fn push_memory(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemorySyncReport> {
    sync_memory(mirror_dir, remote, remote_dir).await
}
#[allow(dead_code)]
pub async fn push_wiki(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemorySyncReport> {
    sync_memory(mirror_dir, remote, remote_dir).await
}
#[allow(dead_code)]
pub async fn collect_wiki(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemoryCollectReport> {
    collect_memory(mirror_dir, remote, remote_dir).await
}
#[allow(dead_code)]
pub async fn pull_memory(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemoryCollectReport> {
    collect_memory(mirror_dir, remote, remote_dir).await
}
#[allow(dead_code)]
pub async fn pull_wiki(
    mirror_dir: &Path,
    remote: &Remote,
    remote_dir: &str,
) -> Result<MemoryCollectReport> {
    collect_memory(mirror_dir, remote, remote_dir).await
}

// ---- Iniciar e acompanhar (F6.3) ----

/// Base64 sem crate nova: o prompt viaja num script shell imune a quoting.
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Percent-encoding mínimo para segmentos de path/query do ai-memory.
fn encode_uri(part: &str) -> String {
    let mut out = String::new();
    for byte in part.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// GET com Bearer no ai-memory local (sem crate HTTP: o mesmo tokio do probe). Best-effort
/// na chamada: qualquer falha vira `Err` e o prompt segue sem briefing.
async fn memory_get(port: u16, path: &str, token: Option<&str>) -> Result<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .context("ai-memory fora do ar")?;
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Accept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let text = String::from_utf8_lossy(&response);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .context("resposta HTTP inválida")?;
    let status = head.split(' ').nth(1).unwrap_or("?");
    if status != "200" {
        bail!("ai-memory respondeu {status}");
    }
    Ok(body.to_owned())
}

/// Passo 4 da spec 08: exporta o briefing do projeto no ai-memory local (servidor da VM,
/// F5.0) como texto para o prompt. O token é o do servidor (`<data_dir>/auth-tokens/default`).
pub async fn export_briefing(
    port: u16,
    token: Option<&str>,
    workspace: &str,
    project: &str,
) -> Result<String> {
    let path = format!(
        "/api/v1/workspaces/{}/projects/{}/briefing?limit=10",
        encode_uri(workspace),
        encode_uri(project)
    );
    let body = memory_get(port, &path, token).await?;
    let snapshot: serde_json::Value =
        serde_json::from_str(body.trim()).context("briefing não é JSON")?;
    Ok(hub_core::sleep::briefing_text(&snapshot))
}

/// O ai-jail grava `.ai-jail` na raiz do projeto e o mascara (vazio) dentro do sandbox:
/// versionado, aparece sempre modificado; solto, suja a árvore e trava o modo dormir. O
/// padrão vai no `info/exclude` do repo: vale por clone, nunca é commitado e o jail enxerga
/// (spec 05, F0.5).
const JAIL_CONFIG_EXCLUDE: &str = "/.ai-jail";

/// Põe `JAIL_CONFIG_EXCLUDE` no `info/exclude` do repo se ainda não estiver lá.
pub async fn exclude_jail_config(project_dir: &Path) -> Result<()> {
    exclude_line(project_dir, JAIL_CONFIG_EXCLUDE).await
}

/// Põe `line` no `info/exclude` do repo se ainda não estiver lá (F0.5 e os worktrees, F11.6).
pub async fn exclude_line(project_dir: &Path, line: &str) -> Result<()> {
    // `--git-path` acerta também em worktrees (aí o caminho volta absoluto).
    let rel = git(project_dir, &["rev-parse", "--git-path", "info/exclude"]).await?;
    let path = project_dir.join(rel);
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current.lines().any(|existing| existing.trim() == line) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let sep = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(&path, format!("{current}{sep}{line}\n"))
        .with_context(|| format!("gravando {}", path.display()))
}

/// Árvore limpa é pré-requisito (spec 08, passo 1). Com `wip`, o que está solto vira um
/// commit WIP; sem `wip`, árvore suja é erro (a UI oferece a opção). Devolve `true` se
/// fez o commit WIP.
pub async fn ensure_clean_tree(project_dir: &Path, wip: bool) -> Result<bool> {
    // Antes do status: o `.ai-jail` das sessões não conta como sujeira nem entra no WIP.
    exclude_jail_config(project_dir).await?;
    let status = git(project_dir, &["status", "--porcelain"]).await?;
    if status.is_empty() {
        return Ok(false);
    }
    if !wip {
        bail!("árvore suja — commite as mudanças ou autorize o commit WIP antes de dormir");
    }
    git(project_dir, &["add", "-A"]).await?;
    let mut args = crate::api::hubd_identity_args(project_dir);
    args.extend(["commit", "-q", "-m", "wip: hora de dormir (auto)"].map(str::to_owned));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    git(project_dir, &args).await.context("commit WIP")?;
    Ok(true)
}

/// Data local de hoje (`YYYY-MM-DD`) — o `<data>` do sync e da branch `dorme/<data>`.
pub async fn today() -> Result<String> {
    let output = Command::new("date")
        .arg("+%F")
        .output()
        .await
        .context("date +%F")?;
    let date = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    validate(&date, "data")?;
    Ok(date)
}

/// A sessão tmux `dorme-<projeto>` existe no notebook?
pub async fn is_sleeping(remote: &Remote, project: &str) -> Result<bool> {
    validate(project, "projeto")?;
    let out = remote
        .shell_out(&format!(
            "tmux has-session -t \"=dorme-{project}\" 2>/dev/null && echo RUNNING || true"
        ))
        .await?;
    Ok(out.contains("RUNNING"))
}

/// Estado do dormir para a UI (passo 7 da spec 08): rodando? + tail do log.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SleepStatusReport {
    pub sleeping: bool,
    pub log_tail: Option<String>,
}

pub async fn sleep_status(
    remote: &Remote,
    workdir: &str,
    project: &str,
) -> Result<SleepStatusReport> {
    validate(workdir, "workdir")?;
    validate(project, "projeto")?;
    let wd = shell_dir(workdir);
    let script = format!(
        "wd=\"{wd}\"\n\
         if tmux has-session -t \"=dorme-{project}\" 2>/dev/null; then printf '@@RUNNING\\n'; fi\n\
         printf '@@LOG\\n'\n\
         tail -n 200 \"$wd/logs/{project}.log\" 2>/dev/null || true\n"
    );
    let out = remote.shell_out(&script).await?;
    let sleeping = out.lines().any(|line| line == "@@RUNNING");
    let log_tail = out
        .split_once("@@LOG\n")
        .map(|(_, rest)| rest.trim_end().to_owned())
        .filter(|text| !text.is_empty());
    Ok(SleepStatusReport { sleeping, log_tail })
}

/// Limites por iteração vindos do `.assistente.toml` do projeto (spec 08).
#[derive(Debug, Clone)]
pub struct SleepLimits {
    pub max_minutes: u32,
    pub attempts: u32,
    pub model: String,
}

#[derive(Debug)]
pub enum StartSleepError {
    /// Já existe `dorme-<projeto>` no notebook.
    AlreadyRunning,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for StartSleepError {
    fn from(error: anyhow::Error) -> Self {
        StartSleepError::Other(error)
    }
}

/// Tudo que o hubd precisa para iniciar o dormir num projeto.
pub struct StartSleep<'a> {
    pub project_dir: &'a Path,
    pub remote: &'a Remote,
    /// `<workdir>/projetos/<perfil>/<projeto>` no notebook.
    pub remote_dir: &'a str,
    /// `<workdir>` do notebook (briefings/, logs/, STOP).
    pub workdir: &'a str,
    pub project: &'a str,
    pub date: &'a str,
    pub prompt: &'a str,
    pub limits: SleepLimits,
    /// Tarefas do quadro (F11.4): o `dorme` puxa uma por iteração em vez do backlog.
    pub quadro: bool,
    /// Produção: `dorme` (PATH do notebook). Testes: caminho do script no repo.
    pub dorme_command: &'a str,
    /// Env extra do tmux (testes: `AGENTE_OPENCODE` falso, `AGENTE_JAIL=""`).
    pub extra_env: &'a [(String, String)],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StartedSleep {
    pub date: String,
    pub session: String,
    pub remote_dir: String,
    pub prompt_file: String,
    pub log_file: String,
}

/// Passos 2–6 da spec 08: sync (F6.2), prompt escrito em `briefings/`, `dorme` num tmux
/// do notebook com os limites do projeto. O PC pode desligar depois disso.
pub async fn start_sleep(opts: StartSleep<'_>) -> Result<StartedSleep, StartSleepError> {
    let fail = |e: anyhow::Error| StartSleepError::Other(e);
    for (part, what) in [
        (opts.project, "projeto"),
        (opts.workdir, "workdir"),
        (opts.remote_dir, "caminho remoto"),
        (opts.date, "data"),
        (opts.dorme_command, "comando dorme"),
    ] {
        validate(part, what).map_err(fail)?;
    }
    let session = format!("dorme-{}", opts.project);
    if is_sleeping(opts.remote, opts.project).await.map_err(fail)? {
        return Err(StartSleepError::AlreadyRunning);
    }

    // Clone criado/atualizado e branch dorme/<data> pronta (F6.2).
    sync_project(opts.project_dir, opts.remote, opts.remote_dir, opts.date)
        .await
        .map_err(fail)?;

    // Prompt por base64: imune a qualquer quoting do texto.
    let wd = shell_dir(opts.workdir);
    let prompt_file = format!("{wd}/briefings/{}.md", opts.project);
    let log_file = format!("{wd}/logs/{}.log", opts.project);
    let stop_file = format!("{wd}/STOP");
    let b64 = base64(opts.prompt.as_bytes());
    opts.remote
        .shell(&format!(
            "mkdir -p \"{wd}/briefings\" \"{wd}/logs\"\nprintf '%s' '{b64}' | base64 -d > \"{prompt_file}\"\n"
        ))
        .await
        .map_err(fail)?;

    // tmux com env por flag (-e): nada de quoting do prompt/valores no comando.
    let remote_dir = shell_dir(opts.remote_dir);
    let mut envs = vec![
        format!("-e \"AGENTE_PROMPT_FILE={prompt_file}\""),
        format!("-e \"AGENTE_LOG={log_file}\""),
        format!("-e \"AGENTE_STOP={stop_file}\""),
        format!("-e \"AGENTE_MAX_MIN={}\"", opts.limits.max_minutes),
        format!("-e \"AGENTE_TENTATIVAS={}\"", opts.limits.attempts),
        // O instalador do notebook põe o dorme/opencode/ai-jail em ~/.local/bin e
        // ~/.opencode/bin, fora do PATH de shells não interativos (spec 07).
        "-e \"PATH=$HOME/.local/bin:$HOME/.opencode/bin:$PATH\"".to_owned(),
    ];
    if opts.quadro {
        envs.push(format!("-e \"AGENTE_QUADRO={}\"", hub_core::kanban::FILE));
    }
    if !opts.limits.model.is_empty() {
        validate(&opts.limits.model, "modelo").map_err(fail)?;
        envs.push(format!("-e \"AGENTE_MODEL={}\"", opts.limits.model));
    }
    for (key, value) in opts.extra_env {
        validate(key, "nome de variável").map_err(fail)?;
        // Valor vazio é legítimo (AGENTE_JAIL="" desliga o sandbox nos testes).
        if !value.is_empty() {
            validate(value, "valor de variável").map_err(fail)?;
        }
        envs.push(format!("-e \"{key}={value}\""));
    }
    opts.remote
        .shell(&format!(
            "tmux new-session -d -s \"{session}\" -c \"{remote_dir}\" {} \"{dorme}\"\n",
            envs.join(" "),
            dorme = opts.dorme_command
        ))
        .await
        .map_err(fail)?;

    Ok(StartedSleep {
        date: opts.date.to_owned(),
        session,
        remote_dir: opts.remote_dir.to_owned(),
        // Para a UI: o caminho como na config (`~/…`), não a expansão do shell remoto.
        prompt_file: format!("{}/briefings/{}.md", opts.workdir, opts.project),
        log_file: format!("{}/logs/{}.log", opts.workdir, opts.project),
    })
}

/// F6.4: cria o arquivo STOP no notebook. O `dorme` confere no topo de cada iteração: a
/// tarefa em curso termina e o loop para. `dorme` apaga o STOP ao iniciar, então pedir
/// parada sem nada rodando é inofensivo.
pub async fn request_stop(remote: &Remote, stop_file: &str) -> Result<()> {
    validate(stop_file, "caminho do STOP")?;
    let path = shell_dir(stop_file);
    remote
        .shell(&format!(
            "mkdir -p \"$(dirname \"{path}\")\"\ntouch \"{path}\"\n"
        ))
        .await
        .context("criando o arquivo STOP no notebook")
}

/// Helpers de git para os testes do modo dormir; os testes de rota em `api.rs` também usam.
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Síncrono de propósito: só nos testes, para montar os repos.
    pub(crate) fn git_sync(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    pub(crate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hubd-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Repo git com um commit inicial na branch main.
    pub(crate) fn init_repo(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        git_sync(dir, &["init", "-q", "-b", "main"]);
        git_sync(dir, &["config", "user.email", "pc@assistente.local"]);
        git_sync(dir, &["config", "user.name", "PC"]);
        git_sync(dir, &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.join("a.txt"), "1").unwrap();
        git_sync(dir, &["add", "."]);
        git_sync(dir, &["commit", "-q", "-m", "c1"]);
    }

    pub(crate) fn repo_pc(name: &str) -> PathBuf {
        let dir = scratch(name);
        init_repo(&dir);
        dir
    }

    pub(crate) fn commit(dir: &Path, file: &str, msg: &str) -> String {
        std::fs::write(dir.join(file), msg).unwrap();
        git_sync(dir, &["add", "."]);
        git_sync(dir, &["commit", "-q", "-m", msg]);
        git_sync(dir, &["rev-parse", "HEAD"])
    }

    #[tokio::test]
    async fn first_sync_creates_the_clone_on_the_sleep_branch() {
        let pc = repo_pc("first");
        let nb = scratch("first-nb");
        std::fs::remove_dir_all(&nb).unwrap(); // o clone não existe ainda
        let nb_str = nb.to_str().unwrap();

        let report = sync_project(&pc, &Remote::Local, nb_str, "2026-10-20")
            .await
            .unwrap();

        assert_eq!(report.base, "main");
        assert_eq!(report.head, git_sync(&pc, &["rev-parse", "HEAD"]));
        assert_eq!(report.sleep_branch, "dorme/2026-10-20");
        // O notebook tem exatamente o HEAD do PC, na branch de trabalho, com os arquivos.
        assert_eq!(git_sync(&nb, &["rev-parse", "HEAD"]), report.head);
        assert_eq!(
            git_sync(&nb, &["branch", "--show-current"]),
            "dorme/2026-10-20"
        );
        assert_eq!(std::fs::read_to_string(nb.join("a.txt")).unwrap(), "1");
        assert_eq!(
            git_sync(&nb, &["rev-parse", "refs/remotes/pc/main"]),
            report.head
        );
        std::fs::remove_dir_all(&pc).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }

    #[tokio::test]
    async fn resync_follows_pc_and_never_touches_notebook_branches() {
        let pc = repo_pc("resync");
        let nb = scratch("resync-nb");
        std::fs::remove_dir_all(&nb).unwrap();
        let nb_str = nb.to_str().unwrap();
        sync_project(&pc, &Remote::Local, nb_str, "2026-10-20")
            .await
            .unwrap();

        // Branch do próprio notebook, com commit próprio.
        git_sync(&nb, &["config", "user.email", "nb@assistente.local"]);
        git_sync(&nb, &["config", "user.name", "Notebook"]);
        git_sync(&nb, &["switch", "-q", "-c", "minha-branch"]);
        commit(&nb, "nb.txt", "trabalho do notebook");

        // PC anda (commit novo) e o sync do dia seguinte segue, sem tocar em minha-branch.
        let head2 = commit(&pc, "b.txt", "c2");
        let report = sync_project(&pc, &Remote::Local, nb_str, "2026-10-21")
            .await
            .unwrap();
        assert_eq!(report.head, head2);
        assert_eq!(git_sync(&nb, &["rev-parse", "HEAD"]), head2);
        assert_eq!(
            git_sync(&nb, &["branch", "--show-current"]),
            "dorme/2026-10-21"
        );
        assert_eq!(
            git_sync(&nb, &["show", "minha-branch:nb.txt"]),
            "trabalho do notebook"
        );

        // PC volta no tempo (reset): o force deixa o notebook com exatamente o HEAD.
        git_sync(&pc, &["reset", "-q", "--hard", "HEAD~1"]);
        let head_back = git_sync(&pc, &["rev-parse", "HEAD"]);
        sync_project(&pc, &Remote::Local, nb_str, "2026-10-22")
            .await
            .unwrap();
        assert_eq!(git_sync(&nb, &["rev-parse", "HEAD"]), head_back);
        assert_eq!(
            git_sync(&nb, &["show", "minha-branch:nb.txt"]),
            "trabalho do notebook"
        );
        std::fs::remove_dir_all(&pc).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }

    /// Quantas vezes o padrão do `.ai-jail` aparece no `info/exclude` do repo.
    fn jail_exclude_lines(repo: &Path) -> usize {
        std::fs::read_to_string(repo.join(".git/info/exclude"))
            .unwrap_or_default()
            .lines()
            .filter(|line| *line == JAIL_CONFIG_EXCLUDE)
            .count()
    }

    #[tokio::test]
    async fn jail_config_does_not_dirty_the_pc_tree() {
        let pc = repo_pc("jail-pc");
        std::fs::write(pc.join(".ai-jail"), "command = [\"opencode\"]\n").unwrap();

        // Só o `.ai-jail` solto: árvore limpa, nada de WIP.
        assert!(!ensure_clean_tree(&pc, false).await.unwrap());
        exclude_jail_config(&pc).await.unwrap();
        assert_eq!(jail_exclude_lines(&pc), 1, "o exclude não se repete");

        // O exclude não esconde o resto: outro arquivo solto continua sujando.
        std::fs::write(pc.join("solto.txt"), "x").unwrap();
        assert!(ensure_clean_tree(&pc, false).await.is_err());

        // Worktree: `--git-path` aponta para o `info/exclude` comum do repo.
        let wt = scratch("jail-wt");
        std::fs::remove_dir_all(&wt).unwrap();
        git_sync(&pc, &["worktree", "add", "-q", wt.to_str().unwrap()]);
        std::fs::write(wt.join(".ai-jail"), "").unwrap();
        exclude_jail_config(&wt).await.unwrap();
        assert_eq!(git_sync(&wt, &["status", "--porcelain"]), "");
        assert_eq!(jail_exclude_lines(&pc), 1);

        std::fs::remove_dir_all(&wt).unwrap();
        std::fs::remove_dir_all(&pc).unwrap();
    }

    #[tokio::test]
    async fn jail_config_does_not_dirty_the_notebook_clone() {
        let pc = repo_pc("jail-sync");
        let nb = scratch("jail-sync-nb");
        std::fs::remove_dir_all(&nb).unwrap();
        let nb_str = nb.to_str().unwrap();
        sync_project(&pc, &Remote::Local, nb_str, "2026-10-20")
            .await
            .unwrap();

        // O ai-jail do `dorme` cria o arquivo no clone: o `git status --porcelain` do loop
        // continua vazio.
        std::fs::write(nb.join(".ai-jail"), "command = [\"opencode\"]\n").unwrap();
        assert_eq!(git_sync(&nb, &["status", "--porcelain"]), "");

        // Resync (noite seguinte) não duplica a linha.
        sync_project(&pc, &Remote::Local, nb_str, "2026-10-21")
            .await
            .unwrap();
        assert_eq!(jail_exclude_lines(&nb), 1);
        std::fs::remove_dir_all(&pc).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }

    #[test]
    fn shell_dir_expands_only_the_leading_tilde() {
        assert_eq!(shell_dir("~/assistente/STOP"), "$HOME/assistente/STOP");
        assert_eq!(shell_dir("~"), "$HOME");
        assert_eq!(shell_dir("/abs/path"), "/abs/path");
    }

    #[tokio::test]
    async fn request_stop_creates_the_file_and_the_parent_dir() {
        let dir = scratch("stop");
        let stop = dir.join("logs/STOP");
        request_stop(&Remote::Local, stop.to_str().unwrap())
            .await
            .unwrap();
        assert!(stop.is_file());
        // Metacaracteres não chegam ao script remoto.
        assert!(
            request_stop(&Remote::Local, "/tmp/x$(reboot)")
                .await
                .is_err()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn detached_head_and_bad_inputs_are_rejected() {
        let pc = repo_pc("erros");
        let nb = scratch("erros-nb");

        git_sync(&pc, &["checkout", "-q", "--detach"]);
        let err = sync_project(&pc, &Remote::Local, nb.to_str().unwrap(), "2026-10-20")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("detached"), "{err}");
        git_sync(&pc, &["switch", "-q", "main"]);

        // Metacaracteres no caminho/data não chegam ao script remoto.
        assert!(
            sync_project(&pc, &Remote::Local, "/tmp/x$(whoami)", "2026-10-20")
                .await
                .is_err()
        );
        assert!(
            sync_project(&pc, &Remote::Local, nb.to_str().unwrap(), "a b; rm -rf /")
                .await
                .is_err()
        );
        std::fs::remove_dir_all(&pc).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }

    /// Noite de sono no "notebook": branch agente/* com commit, merge --no-ff na
    /// dorme/<data> e RELATORIO.md sem commitar (contrato do agente, spec 08).
    fn noite_no_notebook(nb: &Path, date: &str) {
        git_sync(nb, &["config", "user.email", "nb@assistente.local"]);
        git_sync(nb, &["config", "user.name", "Notebook"]);
        git_sync(nb, &["switch", "-q", "-c", "agente/f9.9"]);
        commit(nb, "feat.txt", "feat: tarefa da noite");
        git_sync(nb, &["switch", "-q", &format!("dorme/{date}")]);
        git_sync(
            nb,
            &[
                "merge",
                "-q",
                "--no-ff",
                "agente/f9.9",
                "-m",
                "merge: f9.9 (modo-dormir)",
            ],
        );
        std::fs::write(nb.join("RELATORIO.md"), "fila esgotada para o agente\n").unwrap();
    }

    #[tokio::test]
    async fn collect_fetches_relatorio_and_merges_fast_forward() {
        let pc = repo_pc("collect");
        let nb = scratch("collect-nb");
        std::fs::remove_dir_all(&nb).unwrap();
        let nb_str = nb.to_str().unwrap();
        sync_project(&pc, &Remote::Local, nb_str, "2026-10-20")
            .await
            .unwrap();
        noite_no_notebook(&nb, "2026-10-20");

        let report = collect_project(&pc, &Remote::Local, nb_str, "2026-10-20")
            .await
            .unwrap();
        assert_eq!(report.base, "main");
        assert_eq!(report.sleep_branch, "dorme/2026-10-20");
        assert!(report.fast_forward);
        let subjects: Vec<_> = report.commits.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["merge: f9.9 (modo-dormir)"]);
        assert_eq!(
            report.relatorio.as_deref(),
            Some("fila esgotada para o agente")
        );
        // As refs ficaram em refs/remotes/notebook/*, como no passo 8 da spec.
        git_sync(
            &pc,
            &["rev-parse", "refs/remotes/notebook/dorme/2026-10-20"],
        );
        git_sync(&pc, &["rev-parse", "refs/remotes/notebook/agente/f9.9"]);

        // Merge com um clique: fast-forward traz o trabalho para a árvore do PC.
        merge_sleep_branch(&pc, "2026-10-20").await.unwrap();
        assert_eq!(
            git_sync(&pc, &["rev-parse", "HEAD"]),
            git_sync(&nb, &["rev-parse", "HEAD"])
        );
        assert_eq!(
            std::fs::read_to_string(pc.join("feat.txt")).unwrap(),
            "feat: tarefa da noite"
        );

        // A base andou durante a noite (commit no PC e no notebook): sem fast-forward,
        // e o merge não muda nada.
        commit(&pc, "c3.txt", "c3");
        commit(&nb, "c4.txt", "c4");
        let report = collect_project(&pc, &Remote::Local, nb_str, "2026-10-20")
            .await
            .unwrap();
        assert!(!report.fast_forward);
        let before = git_sync(&pc, &["rev-parse", "HEAD"]);
        assert!(merge_sleep_branch(&pc, "2026-10-20").await.is_err());
        assert_eq!(git_sync(&pc, &["rev-parse", "HEAD"]), before);

        std::fs::remove_dir_all(&pc).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }

    #[tokio::test]
    async fn collect_without_a_sleep_night_is_an_error() {
        let pc = repo_pc("collect-vazio");
        let nb = scratch("collect-vazio-nb");
        std::fs::remove_dir_all(&nb).unwrap();
        let nb_str = nb.to_str().unwrap();
        sync_project(&pc, &Remote::Local, nb_str, "2026-10-20")
            .await
            .unwrap();
        // Data em que não houve dormir: o fetch não acha a branch.
        let err = collect_project(&pc, &Remote::Local, nb_str, "2026-01-01")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("2026-01-01"), "{err}");
        // Merge de ref que nunca foi recolhida também falha.
        assert!(merge_sleep_branch(&pc, "2026-10-20").await.is_err());
        std::fs::remove_dir_all(&pc).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }

    #[tokio::test]
    async fn memory_sync_pushes_wiki_to_notebook() {
        let pc = scratch("mem-push-pc");
        let wiki = pc.join("wiki");
        std::fs::create_dir_all(&wiki).unwrap();
        git_sync(&wiki, &["init", "-q", "-b", "main"]);
        git_sync(&wiki, &["config", "user.email", "pc@assistente.local"]);
        git_sync(&wiki, &["config", "user.name", "PC"]);
        git_sync(&wiki, &["config", "commit.gpgsign", "false"]);
        std::fs::write(wiki.join("nota.md"), "olá").unwrap();
        git_sync(&wiki, &["add", "."]);
        git_sync(&wiki, &["commit", "-q", "-m", "c1"]);

        let nb = scratch("mem-push-nb");
        std::fs::remove_dir_all(&nb).unwrap();
        let nb_str = nb.to_str().unwrap();

        // Passando o mirror_dir (pai do wiki): resolve para wiki/.
        let report = sync_memory(&pc, &Remote::Local, nb_str).await.unwrap();
        assert_eq!(report.head, git_sync(&wiki, &["rev-parse", "HEAD"]));
        assert_eq!(git_sync(&nb, &["rev-parse", "HEAD"]), report.head);
        assert_eq!(std::fs::read_to_string(nb.join("nota.md")).unwrap(), "olá");

        // Idempotência: resync não falha.
        std::fs::write(wiki.join("nota2.md"), "x").unwrap();
        git_sync(&wiki, &["add", "."]);
        git_sync(&wiki, &["commit", "-q", "-m", "c2"]);
        sync_memory(&pc, &Remote::Local, nb_str).await.unwrap();
        assert_eq!(
            git_sync(&nb, &["rev-parse", "HEAD"]),
            git_sync(&wiki, &["rev-parse", "HEAD"])
        );

        std::fs::remove_dir_all(&pc).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }

    #[tokio::test]
    async fn memory_collect_fetches_notebook_changes() {
        let pc = scratch("mem-collect-pc");
        let wiki = pc.join("wiki");
        std::fs::create_dir_all(&wiki).unwrap();
        git_sync(&wiki, &["init", "-q", "-b", "main"]);
        git_sync(&wiki, &["config", "user.email", "pc@assistente.local"]);
        git_sync(&wiki, &["config", "user.name", "PC"]);
        git_sync(&wiki, &["config", "commit.gpgsign", "false"]);
        std::fs::write(wiki.join("a.md"), "1").unwrap();
        git_sync(&wiki, &["add", "."]);
        git_sync(&wiki, &["commit", "-q", "-m", "c1"]);

        let nb = scratch("mem-collect-nb");
        std::fs::remove_dir_all(&nb).unwrap();
        let nb_str = nb.to_str().unwrap();
        sync_memory(&pc, &Remote::Local, nb_str).await.unwrap();

        // Notebook (agente noturno) commita uma nota.
        git_sync(&nb, &["config", "user.email", "nb@assistente.local"]);
        git_sync(&nb, &["config", "user.name", "NB"]);
        std::fs::write(nb.join("noite.md"), "nota da noite").unwrap();
        git_sync(&nb, &["add", "."]);
        git_sync(&nb, &["commit", "-q", "-m", "nota noturna"]);

        let report = collect_memory(&pc, &Remote::Local, nb_str).await.unwrap();
        assert!(report.fast_forward);
        // Merge traz a nota para o PC.
        merge_memory(&pc).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(wiki.join("noite.md")).unwrap(),
            "nota da noite"
        );

        std::fs::remove_dir_all(&pc).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }

    #[tokio::test]
    async fn memory_sync_works_when_mirror_dir_is_the_repo() {
        // mirror_dir pode ser o próprio repo git.
        let repo = scratch("mem-direct-repo");
        std::fs::remove_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        git_sync(&repo, &["init", "-q", "-b", "main"]);
        git_sync(&repo, &["config", "user.email", "pc@assistente.local"]);
        git_sync(&repo, &["config", "user.name", "PC"]);
        git_sync(&repo, &["config", "commit.gpgsign", "false"]);
        std::fs::write(repo.join("x.md"), "x").unwrap();
        git_sync(&repo, &["add", "."]);
        git_sync(&repo, &["commit", "-q", "-m", "c1"]);

        let nb = scratch("mem-direct-nb");
        std::fs::remove_dir_all(&nb).unwrap();
        sync_memory(&repo, &Remote::Local, nb.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(
            git_sync(&nb, &["rev-parse", "HEAD"]),
            git_sync(&repo, &["rev-parse", "HEAD"])
        );

        std::fs::remove_dir_all(&repo).unwrap();
        std::fs::remove_dir_all(&nb).unwrap();
    }
}
