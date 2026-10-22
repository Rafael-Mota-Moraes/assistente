//! Diagnóstico da VM lido pelo agente (F14.1). A API dos agentes (`agent_router`) ganha uma
//! rota de leitura: `POST /v1/diagnostico {"comando": "…"}`. Os comandos são uma lista
//! fechada, escrita em Rust: o agente não escolhe o que roda, só qual leitura fazer (nada de
//! shell livre). Como a leitura não escreve nada, ela não passa pela aprovação do app (F9.7).
//! Spec: docs/specs/02-hubd-e-sessoes.md ("API dos agentes")

// Os handlers devolvem `Response` como o resto de `api.rs`; o `Err` grande é o preço disso.
#![allow(clippy::result_large_err)]

use super::*;
use hub_core::{profile_data_dir, tmux_session_prefix};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::process::Command;

/// Teto da saída: 64 KiB (o mesmo do `fora-do jail`).
const MAX_OUTPUT: usize = 64 * 1024;
/// Linhas do journal quando o agente não pede um número.
const DEFAULT_JOURNAL_LINES: u32 = 200;
/// Teto das linhas do journal (o `journalctl` também corta).
const MAX_JOURNAL_LINES: u32 = 5_000;
/// Quantas linhas do histórico o `capture-pane` devolve.
const PANE_LINES: i32 = -200;

/// Os comandos de diagnóstico que existem. O texto do agente só escolhe um deles.
const NOMES: &[&str] = &[
    "journal-hubd",
    "tmux-ls",
    "tmux-pane",
    "explorar",
    "claude-login",
];

/// Um comando de diagnóstico: a lista é fechada e cada um sabe como roda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comando {
    /// As últimas linhas do journal do hubd (`journalctl -u hubd`).
    JournalHubd { linhas: u32 },
    /// As sessões do tmux.
    TmuxLs,
    /// A saída do painel de uma sessão do projeto.
    TmuxPane { sessao: String },
    /// Os prompts de `explorar/` do perfil (com o tamanho de cada um).
    Explorar,
    /// Onde o `claude` está no PATH de login do perfil.
    ClaudeLogin,
}

impl Comando {
    /// O que o agente escreveu vira um comando. Fora da lista, é recusado (400).
    pub fn parse(texto: &str) -> Result<Self, String> {
        let mut partes = texto.split_whitespace();
        let Some(nome) = partes.next() else {
            return Err(format!("comando vazio; use um de: {}", NOMES.join(", ")));
        };
        let resto: Vec<&str> = partes.collect();
        match nome {
            "journal-hubd" => {
                let linhas = match resto.as_slice() {
                    [] => DEFAULT_JOURNAL_LINES,
                    [n] => n
                        .parse::<u32>()
                        .map_err(|_| format!("journal-hubd: número de linhas inválido: {n}"))?,
                    _ => {
                        return Err(
                            "journal-hubd: no máximo um argumento (o número de linhas)".to_owned()
                        );
                    }
                };
                Ok(Self::JournalHubd {
                    linhas: linhas.clamp(1, MAX_JOURNAL_LINES),
                })
            }
            "tmux-ls" => {
                sem_argumento(&resto, "tmux-ls")?;
                Ok(Self::TmuxLs)
            }
            "explorar" => {
                sem_argumento(&resto, "explorar")?;
                Ok(Self::Explorar)
            }
            "claude-login" => {
                sem_argumento(&resto, "claude-login")?;
                Ok(Self::ClaudeLogin)
            }
            "tmux-pane" => {
                let [sessao] = resto.as_slice() else {
                    return Err(
                        "tmux-pane: falta a sessão (ex.: tmux-pane pessoal--assistente--claude-1)"
                            .to_owned(),
                    );
                };
                if sessao.is_empty()
                    || !sessao
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err("tmux-pane: nome de sessão inválido".to_owned());
                }
                Ok(Self::TmuxPane {
                    sessao: (*sessao).to_owned(),
                })
            }
            outro => Err(format!(
                "comando de diagnóstico desconhecido: {outro}; use um de: {}",
                NOMES.join(", ")
            )),
        }
    }

    /// O comando como texto (o que a resposta devolve).
    fn texto(&self) -> String {
        match self {
            Self::JournalHubd { linhas } => format!("journal-hubd {linhas}"),
            Self::TmuxLs => "tmux-ls".to_owned(),
            Self::TmuxPane { sessao } => format!("tmux-pane {sessao}"),
            Self::Explorar => "explorar".to_owned(),
            Self::ClaudeLogin => "claude-login".to_owned(),
        }
    }
}

/// Comando que não recebe argumento.
fn sem_argumento(resto: &[&str], nome: &str) -> Result<(), String> {
    if resto.is_empty() {
        Ok(())
    } else {
        Err(format!("{nome}: não recebe argumento"))
    }
}

/// O que o comando devolveu.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostico {
    /// O comando como o agente escreveu.
    pub comando: String,
    /// A saída do comando.
    pub saida: String,
    /// O código de saída (`None` = o comando nem chegou a rodar).
    pub codigo: Option<i32>,
    /// O que o comando reclamou (stderr).
    pub erro: Option<String>,
    /// A saída passou do teto e foi cortada (fica o fim).
    pub truncado: bool,
}

#[derive(Deserialize)]
pub(super) struct Pedido {
    comando: String,
}

/// POST /v1/diagnostico (API dos agentes): leitura de diagnóstico, sem aprovação.
pub(super) async fn agent_diagnostico(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(pedido): Json<Pedido>,
) -> Result<Json<Diagnostico>, Response> {
    let sessao = agent_session(&state, &headers).await?;
    let comando =
        Comando::parse(&pedido.comando).map_err(|e| error(StatusCode::BAD_REQUEST, &e))?;
    let resultado = roda(&state, &sessao, &comando).await?;
    tracing::info!("diagnóstico de {}: {}", sessao.id, pedido.comando.trim());
    Ok(Json(resultado))
}

/// Roda o comando escolhido (leitura só) e monta a resposta.
async fn roda(
    state: &AppState,
    sessao: &SessionInfo,
    comando: &Comando,
) -> Result<Diagnostico, Response> {
    let (saida, codigo, erro) = match comando {
        Comando::JournalHubd { linhas } => {
            let mut command = Command::new("journalctl");
            command.args(["--no-pager", "-u", "hubd", "-n", &linhas.to_string()]);
            let out = roda_processo(command).await;
            (out.saida, out.codigo, out.erro)
        }
        Comando::TmuxLs => match state.0.tmux.list().await {
            Ok(sessoes) => {
                let texto = sessoes
                    .iter()
                    .map(|s| format!("{}\t{}\t{}\t{}", s.id, s.profile, s.project, s.agent.id()))
                    .collect::<Vec<_>>()
                    .join("\n");
                (texto, Some(0), None)
            }
            Err(e) => (String::new(), Some(1), Some(e)),
        },
        Comando::TmuxPane { sessao: alvo } => {
            let texto = pane_do_projeto(state, sessao, alvo)
                .await
                .map_err(|(status, mensagem)| error(status, &mensagem))?;
            (texto, Some(0), None)
        }
        Comando::Explorar => {
            listar_explorar(&profile_data_dir(&state.0.home, &sessao.profile).join("explorar"))
        }
        Comando::ClaudeLogin => {
            let mut command = Command::new("bash");
            command.args(["-l", "-c", "command -v claude"]);
            let out = roda_processo(command).await;
            let mut erro = out.erro;
            if out.saida.trim().is_empty() && erro.is_none() {
                erro = Some("o `claude` não está no PATH de login deste perfil".to_owned());
            }
            (out.saida, out.codigo, erro)
        }
    };
    let (saida, cortada) = corta(&saida, MAX_OUTPUT);
    let (erro, erro_cortado) = match erro {
        Some(texto) => {
            let (texto, cortada) = corta(&texto, MAX_OUTPUT);
            (Some(texto), cortada)
        }
        None => (None, false),
    };
    Ok(Diagnostico {
        comando: comando.texto(),
        saida,
        codigo,
        erro,
        truncado: cortada || erro_cortado,
    })
}

/// A saída de um processo: stdout, stderr e o código de saída.
struct Saida {
    saida: String,
    erro: Option<String>,
    codigo: Option<i32>,
}

/// Roda um processo e recolhe o que ele disse (sem shell: cada comando é uma lista de argv).
async fn roda_processo(mut command: Command) -> Saida {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    match command.output().await {
        Ok(out) => Saida {
            saida: String::from_utf8_lossy(&out.stdout).into_owned(),
            erro: (!out.stderr.is_empty())
                .then(|| String::from_utf8_lossy(&out.stderr).trim().to_owned()),
            codigo: out.status.code(),
        },
        Err(e) => Saida {
            saida: String::new(),
            erro: Some(format!("não consegui rodar o comando: {e}")),
            codigo: None,
        },
    }
}

/// A saída do painel de uma sessão do projeto. A sessão viva precisa ser deste projeto; a
/// morta (que ainda deixou o painel, F14.3) é aceita pelo prefixo do nome.
async fn pane_do_projeto(
    state: &AppState,
    sessao: &SessionInfo,
    alvo: &str,
) -> Result<String, (StatusCode, String)> {
    let sessoes = state.0.tmux.list().await.unwrap_or_default();
    if let Some(outra) = sessoes.iter().find(|s| s.id == alvo) {
        if outra.profile != sessao.profile || outra.project != sessao.project {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("a sessão {alvo} não é deste projeto"),
            ));
        }
    } else if !alvo.starts_with(&tmux_session_prefix(&sessao.profile, &sessao.project)) {
        return Err((
            StatusCode::NOT_FOUND,
            format!("sessão não encontrada: {alvo}"),
        ));
    }
    state
        .0
        .tmux
        .capture_pane(alvo, PANE_LINES)
        .await
        .map_err(|e| {
            (
                StatusCode::NOT_FOUND,
                format!("não consegui ler a sessão {alvo}: {e}"),
            )
        })
}

/// Os prompts de `explorar/` do perfil, com o tamanho de cada um.
fn listar_explorar(dir: &std::path::Path) -> (String, Option<i32>, Option<String>) {
    let entradas = match std::fs::read_dir(dir) {
        Ok(entradas) => entradas,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                format!("nenhum prompt de explorar em {}", dir.display()),
                Some(0),
                None,
            );
        }
        Err(e) => {
            return (
                String::new(),
                Some(1),
                Some(format!("não consegui ler {}: {e}", dir.display())),
            );
        }
    };
    let mut linhas: Vec<String> = entradas
        .flatten()
        .map(|entrada| {
            let nome = entrada.file_name().to_string_lossy().into_owned();
            let tamanho = entrada.metadata().map(|m| m.len()).unwrap_or(0);
            format!("{nome}\t{tamanho} bytes")
        })
        .collect();
    linhas.sort();
    (linhas.join("\n"), Some(0), None)
}

/// Corta a saída no teto. Fica o fim, que é onde costuma estar o erro.
fn corta(texto: &str, teto: usize) -> (String, bool) {
    if texto.len() <= teto {
        return (texto.to_owned(), false);
    }
    let mut inicio = texto.len() - teto;
    while !texto.is_char_boundary(inicio) {
        inicio += 1;
    }
    (texto[inicio..].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_aceita_so_a_lista() {
        assert_eq!(
            Comando::parse("journal-hubd").unwrap(),
            Comando::JournalHubd {
                linhas: DEFAULT_JOURNAL_LINES
            }
        );
        assert_eq!(
            Comando::parse("journal-hubd 50").unwrap(),
            Comando::JournalHubd { linhas: 50 }
        );
        assert_eq!(Comando::parse("tmux-ls").unwrap(), Comando::TmuxLs);
        assert_eq!(
            Comando::parse("tmux-pane pessoal--x--claude-1").unwrap(),
            Comando::TmuxPane {
                sessao: "pessoal--x--claude-1".to_owned()
            }
        );
        assert_eq!(Comando::parse("explorar").unwrap(), Comando::Explorar);
        assert_eq!(
            Comando::parse("claude-login").unwrap(),
            Comando::ClaudeLogin
        );
        // Espaço a mais não muda nada.
        assert_eq!(Comando::parse("  tmux-ls  ").unwrap(), Comando::TmuxLs);
    }

    #[test]
    fn parser_recusa_o_que_esta_fora_da_lista() {
        for texto in [
            "",
            "rm -rf /",
            "journalctl -u hubd",
            "bash -c 'echo oi'",
            "tmux",
            "tmux-ls; rm -rf /",
        ] {
            assert!(Comando::parse(texto).is_err(), "aceitou: {texto:?}");
        }
        // Argumentos fora do que o comando aceita.
        assert!(Comando::parse("journal-hubd abc").is_err());
        assert!(Comando::parse("journal-hubd 1 2").is_err());
        assert!(Comando::parse("tmux-ls agora").is_err());
        assert!(Comando::parse("tmux-pane").is_err());
        assert!(Comando::parse("tmux-pane a b").is_err());
        assert!(Comando::parse("tmux-pane '; rm -rf /'").is_err());
        assert!(Comando::parse("explorar tudo").is_err());
        assert!(Comando::parse("claude-login --verbose").is_err());
    }

    #[test]
    fn journal_linhas_tem_teto() {
        let Comando::JournalHubd { linhas } = Comando::parse("journal-hubd 999999").unwrap() else {
            panic!("não é journal");
        };
        assert_eq!(linhas, MAX_JOURNAL_LINES);
        let Comando::JournalHubd { linhas } = Comando::parse("journal-hubd 0").unwrap() else {
            panic!("não é journal");
        };
        assert_eq!(linhas, 1);
    }

    #[test]
    fn corte_guarda_o_fim() {
        let (texto, cortada) = corta("0123456789", 4);
        assert_eq!(texto, "6789");
        assert!(cortada);
        let (texto, cortada) = corta("oi", 10);
        assert_eq!(texto, "oi");
        assert!(!cortada);
        // Não corta no meio de um caractere UTF-8.
        let (texto, cortada) = corta("áéíóú", 3);
        assert!(cortada);
        assert_eq!(texto, "ú");
    }
}
