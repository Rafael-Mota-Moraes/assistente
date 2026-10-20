//! Modo dormir, parte git (F6.2): levar o HEAD do PC até o clone no notebook e deixar a
//! branch de trabalho `dorme/<data>` pronta, sem tocar em nada que é do notebook.
//! Spec: docs/specs/08-modo-dormir.md (passos 2 e 3 do fluxo).
//!
//! O hubd roda na VM, onde as raízes dos perfis estão montadas no MESMO caminho do PC
//! (virtiofs), então `git -C <raiz>/<perfil>/<projeto>` aqui vê o repo do PC. O notebook é
//! alcançado pelo alias ssh da config (`Remote::Ssh`); nos testes, `Remote::Local` roda o
//! mesmo script bash num diretório local (dois repos locais simulando PC e notebook).
//!
//! O resto da orquestração (briefing, prompt, tmux remoto) é F6.3.
#![allow(dead_code)] // sync_project entra em uso na rota POST /api/sleep (F6.3)

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Onde fica o "notebook": alias ssh (produção) ou o próprio host (testes).
#[derive(Debug, Clone)]
pub enum Remote {
    Ssh { host: String },
    Local,
}

impl Remote {
    /// Roda um script bash no remoto e devolve o stdout; falha com o stderr na mensagem.
    async fn shell_out(&self, script: &str) -> Result<String> {
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
fn shell_dir(remote_dir: &str) -> String {
    match remote_dir.strip_prefix("~/") {
        Some(rest) => format!("$HOME/{rest}"),
        None if remote_dir == "~" => "$HOME".to_owned(),
        None => remote_dir.to_owned(),
    }
}

/// Branch, data e caminho entram num script shell e num ref do git: nada de metacaracteres.
fn validate(part: &str, what: &str) -> Result<()> {
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
    // Clone criado na primeira vez; repo existente é reutilizado.
    remote
        .shell(&format!(
            "mkdir -p \"{dir}\"\ngit -C \"{dir}\" rev-parse --git-dir >/dev/null 2>&1 || git -C \"{dir}\" init -q -b main\n"
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
}
