//! Trabalhadores do quadro (F11.6): uma sessão do opencode que executa uma tarefa `agente` num
//! worktree próprio, com modelos de reserva. O loop de tentativas e a troca de modelo rodam em
//! `vm/trabalhador`, dentro do jail; aqui ficam a config do projeto, o worktree, o prompt e o
//! registro da execução (para reiniciar). Spec: docs/specs/10-kanban-e-trabalhadores.md

use crate::kanban::Card;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `[workers]` do `.assistente.toml`: modelos do opencode (os "grátis") e limites.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkersConfig {
    /// Ordem de uso: o primeiro é o principal; os outros são reserva quando ele falha. Vazio =
    /// o hubd escolhe entre os grátis do opencode, pelo histórico (F11.10).
    pub models: Vec<String>,
    /// Tentativas por modelo antes de trocar para o próximo.
    pub attempts: u32,
    /// Minutos máximos por tentativa.
    pub max_minutes: u32,
    /// Segundos de espera entre tentativas.
    pub wait_secs: u32,
}

impl Default for WorkersConfig {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            attempts: 3,
            max_minutes: 60,
            wait_secs: 60,
        }
    }
}

/// Registro de uma execução: o que o trabalhador precisa para ser relançado do mesmo jeito.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRun {
    pub task: String,
    pub models: Vec<String>,
    pub attempts: u32,
    pub max_minutes: u32,
    pub wait_secs: u32,
    /// Onde o agente trabalha: `<projeto>/.assistente/worktrees/<tarefa>`.
    pub worktree: PathBuf,
}

/// Arquivos da execução, na pasta de dados do perfil (visível dentro do jail, fora do repo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerFiles {
    pub prompt: PathBuf,
    pub log: PathBuf,
    pub run: PathBuf,
    /// Um `ok` ou `falhou` por tentativa, para o histórico de modelos (F11.10).
    pub outcomes: PathBuf,
}

const MAX_MODEL_LEN: usize = 100;
pub const MAX_MODELS: usize = 5;

/// Nome de modelo sem espaço nem metacaractere: vai para a linha de comando do tmux.
pub fn validate_model(model: &str) -> Result<(), String> {
    let ok = !model.is_empty()
        && model.len() <= MAX_MODEL_LEN
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._/:-".contains(c));
    if ok {
        Ok(())
    } else {
        Err(format!("nome de modelo inválido: {model:?}"))
    }
}

/// Modelos pedidos (principal + reservas), validados e sem repetição.
pub fn checked_models(models: &[String]) -> Result<Vec<String>, String> {
    if models.is_empty() {
        return Err("escolha pelo menos um modelo".to_owned());
    }
    if models.len() > MAX_MODELS {
        return Err(format!("no máximo {MAX_MODELS} modelos"));
    }
    let mut unique: Vec<String> = Vec::new();
    for model in models {
        validate_model(model)?;
        if !unique.contains(model) {
            unique.push(model.clone());
        }
    }
    Ok(unique)
}

/// Sucessos e falhas por modelo (F11.10), somados das execuções do perfil.
pub type Outcomes = HashMap<String, (u32, u32)>;

/// Catálogo de modelos que o opencode mantém (provedor → `models` → modelo → `cost`). É o mesmo
/// que o `/models` da TUI usa, e o próprio opencode o atualiza.
pub fn catalogue_path(home: &Path) -> PathBuf {
    home.join(".cache/opencode/models.json")
}

/// Modelos grátis do opencode, como `opencode/<id>`: do provedor `opencode`, com custo zero de
/// entrada e de saída. Lê o catálogo a cada chamada, então acompanha as mudanças dos modelos.
pub fn free_models(catalogue: &str) -> Vec<String> {
    let Ok(providers) = serde_json::from_str::<serde_json::Value>(catalogue) else {
        return Vec::new();
    };
    let Some(models) = providers
        .get("opencode")
        .and_then(|provider| provider.get("models"))
        .and_then(|models| models.as_object())
    else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    for (id, model) in models {
        let cost = |field: &str| {
            model
                .get("cost")
                .and_then(|cost| cost.get(field))
                .and_then(|value| value.as_f64())
        };
        let name = format!("opencode/{id}");
        // Descontinuado continua no catálogo com custo zero, mas o `opencode run` recusa.
        let deprecated = model.get("status").and_then(|s| s.as_str()) == Some("deprecated");
        if cost("input") == Some(0.0)
            && cost("output") == Some(0.0)
            && !deprecated
            && validate_model(&name).is_ok()
        {
            found.push(name);
        }
    }
    found
}

/// Linhas `modelo ok` e `modelo falhou`, gravadas pelo trabalhador a cada tentativa. Linhas
/// que não batem com isso são ignoradas (um arquivo meio escrito não derruba o histórico).
pub fn parse_outcomes(text: &str) -> Outcomes {
    let mut outcomes = Outcomes::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(model), Some(result), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (ok, failed) = match result {
            "ok" => (1, 0),
            "falhou" => (0, 1),
            _ => continue,
        };
        let entry = outcomes.entry(model.to_owned()).or_insert((0, 0));
        entry.0 += ok;
        entry.1 += failed;
    }
    outcomes
}

/// Ordena os candidatos pela taxa de sucesso, suavizada: sem histórico, um modelo fica no meio.
/// Em empate vale a ordem de descoberta (o sort é estável).
pub fn rank_models(candidates: &[String], outcomes: &Outcomes) -> Vec<String> {
    let score = |model: &String| {
        let (ok, failed) = outcomes.get(model).copied().unwrap_or((0, 0));
        (f64::from(ok) + 1.0) / (f64::from(ok + failed) + 2.0)
    };
    let mut ranked = candidates.to_vec();
    ranked.sort_by(|a, b| score(b).total_cmp(&score(a)));
    ranked
}

/// Prompt da sessão que monta o quadro a partir do backlog em markdown (F11.9).
pub const EXPLORE_PROMPT: &str = include_str!("explorar_prompt.md");

/// Id de tarefa como o quadro gera (`T1`, `T23`). Vai para um caminho de pasta e um nome de
/// branch, então o formato é checado antes de qualquer uso.
pub fn valid_task_id(id: &str) -> bool {
    id.len() > 1
        && id.len() <= 12
        && id.starts_with('T')
        && id[1..].chars().all(|c| c.is_ascii_digit())
}

/// Pasta do worktree da tarefa, dentro do projeto (fora do git pelo `info/exclude`).
pub fn worktree_dir(project: &Path, task: &str) -> PathBuf {
    project.join(".assistente").join("worktrees").join(task)
}

/// Branch da tarefa: o que você revisa e mescla depois.
pub fn branch(task: &str) -> String {
    format!("agente/{task}")
}

/// Pasta do binário do opencode na VM: o provisionamento põe em `~/.opencode/bin` (F1.3). Fica
/// fora do PATH e fora do jail, então o trabalhador precisa dela montada e do caminho absoluto.
pub fn opencode_dir(home: &Path) -> PathBuf {
    home.join(".opencode/bin")
}

/// Binário do opencode que o trabalhador roda: o do instalador se existir, senão o do PATH.
pub fn opencode_bin(home: &Path) -> PathBuf {
    let installed = opencode_dir(home).join("opencode");
    if installed.exists() {
        installed
    } else {
        PathBuf::from("opencode")
    }
}

/// Linha do `info/exclude` que mantém os worktrees fora do `git status` do projeto.
pub const WORKTREES_EXCLUDE: &str = "/.assistente/worktrees/";

/// Arquivos da execução `session` do perfil `profile` (`<data>/trabalhadores/<session>.*`).
pub fn files(profile_data: &Path, session: &str) -> WorkerFiles {
    let dir = profile_data.join("trabalhadores");
    WorkerFiles {
        prompt: dir.join(format!("{session}.md")),
        log: dir.join(format!("{session}.log")),
        run: dir.join(format!("{session}.json")),
        outcomes: dir.join(format!("{session}.modelos")),
    }
}

/// Histórico do perfil: junta os `.modelos` de todas as execuções em `trabalhadores/` (F11.10).
pub fn read_outcomes(profile_data: &Path) -> Outcomes {
    let mut total = Outcomes::new();
    let Ok(entries) = std::fs::read_dir(profile_data.join("trabalhadores")) else {
        return total;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "modelos") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (model, (ok, failed)) in parse_outcomes(&text) {
            let sum = total.entry(model).or_insert((0, 0));
            sum.0 += ok;
            sum.1 += failed;
        }
    }
    total
}

/// Prompt da execução: a tarefa, as regras do trabalhador e o que não fazer.
pub fn render_prompt(card: &Card, branch: &str, instructions: &str) -> String {
    prompt(
        card,
        branch,
        instructions,
        "- Não marque a tarefa como feita nem mexa em `.assistente/kanban.json`: quem marca é o\n  \
         trabalhador, pelo resultado do git.\n",
    )
}

/// Prompt do Claude que termina uma tarefa parada (F11.14, "Terminar com… Claude Code"): a
/// mesma tarefa e o mesmo worktree, e ele mesmo marca o card, porque não há trabalhador.
pub fn render_finish_prompt(card: &Card, branch: &str, instructions: &str) -> String {
    let why = card
        .blocked
        .as_deref()
        .map(|reason| format!(" (motivo: {reason})"))
        .unwrap_or_default();
    let finish = format!(
        "- Um trabalhador do opencode já começou esta tarefa aqui e parou{why}. Antes de tudo,\n  \
         veja o `git log` e o `git status` deste worktree e continue de onde ele parou.\n\
         - Não mexa em `.assistente/kanban.json`. Concluiu, com a árvore limpa, marque a tarefa:\n  \
         `\"$ASSISTENTE_FORA_DO_JAIL\" kanban done {id} --commit \"$(git rev-parse HEAD)\"`.\n  \
         Se não der para concluir, devolva: `\"$ASSISTENTE_FORA_DO_JAIL\" kanban release {id}`.\n",
        id = card.id,
    );
    prompt(card, branch, instructions, &finish)
}

/// Prompt do QA de uma tarefa "feita" (F11.15): pergunta onde juntar, confere o que foi feito
/// contra o pedido e valida (o hubd roda o check e junta, F11.17) ou reabre com o que falta. Não
/// implementa nada.
pub fn render_qa_prompt(card: &Card, branch: &str, instructions: &str) -> String {
    let description = if card.description.is_empty() {
        "(sem descrição: o título é o pedido)".to_owned()
    } else {
        card.description.clone()
    };
    let commit = card
        .commit
        .as_deref()
        .map(|commit| format!(" O trabalhador marcou como feita no commit `{commit}`."))
        .unwrap_or_default();
    format!(
        "# QA da tarefa {id}: {title}\n\n\
         Você é o QA desta tarefa: confere se o que foi feito atende o pedido e, se atender,\n\
         junta. O pedido é este:\n\n\
         {description}\n\n\
         ## Onde está o trabalho\n\n\
         Neste diretório, um worktree do projeto na branch `{branch}`.{commit}\n\n\
         ## Passos\n\n\
         1. Antes de tudo, pergunte ao usuário onde juntar: na `main` ou em outra branch (qual).\n   \
            Espere a resposta. Chame a escolhida de ALVO.\n\
         2. Veja o que foi feito: `git log --oneline ALVO..HEAD` e `git diff ALVO...HEAD`. Confira\n   \
            item por item com o pedido (e o \"Pronto quando\", se houver).\n\
         3. Não atende (falta algo, faz outra coisa): não junte. Rode\n   \
            `\"$ASSISTENTE_FORA_DO_JAIL\" kanban reabrir {id} --nota \"<o que já existe e o que falta>\"`.\n   \
            O card volta ao backlog com a nota na descrição, e o próximo trabalhador continua\n   \
            desta branch. Não implemente o que falta: isso é dele.\n\
         4. Atende: rode `\"$ASSISTENTE_FORA_DO_JAIL\" kanban validar {id} --branch ALVO`. O hubd\n   \
            roda o check do projeto no PC, no commit que a ALVO vai receber (o usuário aprova o\n   \
            pedido no app; o comando espera). Verde: junta `{branch}` na ALVO (merge local, nunca\n   \
            push) e o card vai para \"validado\". Vermelho: nada muda e a resposta traz o fim da\n   \
            saída; reabra (passo 3) com o que quebrou na nota, sem consertar aqui. Negado ou sem\n   \
            resposta: pergunte ao usuário. Conflito: rode `git merge ALVO` aqui, resolva, commite\n   \
            e chame `validar` outra vez.\n\
         5. Termine dizendo ao usuário, em poucas linhas, o resultado e por quê.\n\n\
         ## Regras\n\n\
         - Só corrija aqui o que nasce da junção (conflito). Não faça push nem troque de branch.\n\
         - Não mexa em `.assistente/kanban.json`: o quadro muda pelos comandos acima.\n\
         - Deixe a árvore limpa antes de validar: o que não está commitado não entra.\n\
         {extra}",
        id = card.id,
        title = card.title,
        extra = match instructions.trim() {
            "" => String::new(),
            text => format!("\n## Instruções do projeto\n\n{text}\n"),
        },
    )
}

/// Prompt do QA geral (F11.16): uma sessão na raiz do projeto confere todos os cards "feitos" de
/// uma vez, com uma só pergunta de onde juntar, e valida ou reabre cada um.
pub fn render_qa_all_prompt(cards: &[Card], instructions: &str) -> String {
    let list = card_list(cards);
    format!(
        "# QA geral do quadro\n\n\
         Você é o QA destas {n} tarefas \"feitas\": confere se cada uma atende o pedido e, se\n\
         atender, junta. Você está na raiz do projeto; o trabalho de cada tarefa está na branch\n\
         dela, com um worktree em `.assistente/worktrees/<id>` (se não houver branch, veja o\n\
         commit marcado).\n\n\
         ## Tarefas\n\n\
         {list}\
         ## Passos\n\n\
         1. Antes de tudo, pergunte ao usuário onde juntar: na `main` ou em outra branch (qual).\n   \
            Uma resposta vale para todas. Espere. Chame a escolhida de ALVO.\n\
         2. Uma tarefa por vez, na ordem da lista (a que depende de outra vem depois dela):\n   \
            - Veja o que foi feito: `git log --oneline ALVO..agente/<id>` e\n     \
              `git diff ALVO...agente/<id>`. Confira com o pedido (e o \"Pronto quando\", se houver).\n   \
            - Não atende: `\"$ASSISTENTE_FORA_DO_JAIL\" kanban reabrir <id> --nota \"<o que já existe\n     \
              e o que falta>\"`. Volta ao backlog com a nota; não implemente o que falta.\n   \
            - Atende: `\"$ASSISTENTE_FORA_DO_JAIL\" kanban validar <id> --branch ALVO`. O hubd roda\n     \
              o check do projeto no PC (o usuário aprova; o comando espera). Verde: junta (merge\n     \
              local, nunca push) e o card vai para \"validado\". Vermelho: nada muda e a resposta\n     \
              traz o fim da saída; reabra com o que quebrou na nota. Conflito (a ALVO andou com as\n     \
              anteriores): no worktree da tarefa rode `git merge ALVO`, resolva, commite e chame\n     \
              `validar` de novo.\n\
         3. No fim, um resumo curto ao usuário: o que entrou, o que voltou e por quê.\n\n\
         ## Regras\n\n\
         - Só corrija o que nasce da junção (conflito), e só no worktree da tarefa. Não faça push,\n  \
           não troque a branch da raiz e não commite nela.\n\
         - Não mexa em `.assistente/kanban.json`: o quadro muda pelos comandos acima.\n\
         - Deixe o worktree limpo antes de validar: o que não está commitado não entra.\n\
         - Tarefa que você não conseguir avaliar, deixe como está e diga no resumo.\n\
         {extra}",
        n = cards.len(),
        extra = match instructions.trim() {
            "" => String::new(),
            text => format!("\n## Instruções do projeto\n\n{text}\n"),
        },
    )
}

/// Linha do `info/exclude` que mantém os resumos das revisões (F11.12) fora do git.
pub const REVIEWS_EXCLUDE: &str = "/.assistente/revisoes/";

/// Prompt da revisão em lote (F11.12): uma sessão na raiz do projeto confere os cards "feitos",
/// junta os que passam numa worktree descartável a partir da `main`, roda o check e pede ao hubd
/// para andar com a `main`. O que é defeito de uma branch sozinha volta ao backlog.
pub fn render_review_prompt(cards: &[Card], instructions: &str) -> String {
    let list = card_list(cards);
    format!(
        "# Revisão do quadro\n\n\
         Você revisa estas {n} tarefas \"feitas\" e leva para a `main` as que passam. Ninguém\n\
         acompanha esta sessão: não pergunte nada, faça tudo e termine com o resumo. Você está na\n\
         raiz do projeto; o trabalho de cada tarefa está na branch dela, com um worktree em\n\
         `.assistente/worktrees/<id>` (sem branch, veja o commit marcado).\n\n\
         ## Tarefas\n\n\
         {list}\
         ## Passos\n\n\
         1. Para cada tarefa, leia `git log --oneline main..agente/<id>` e `git diff main...agente/<id>`\n   \
            e confira com o pedido (e o \"Pronto quando\", se houver). Separe: passa; defeito só da\n   \
            branch (falta algo, faz outra coisa, não tem código próprio); defeito que nasce da junção\n   \
            com outra (conflito, dois arquivos novos para a mesma coisa, código que uma esperava da\n   \
            outra).\n\
         2. Defeito só da branch não se corrige aqui. Devolva a tarefa:\n   \
            `\"$ASSISTENTE_FORA_DO_JAIL\" kanban reabrir <id> --nota \"<o que já existe e o que falta>\"`.\n   \
            Ela volta ao backlog com \"Revisão (data): nota\" e o próximo trabalhador continua da branch.\n\
         3. Junte as que passam numa worktree descartável, a partir da `main`:\n   \
            `git worktree add --detach .assistente/worktrees/revisao-$(date +%Y%m%d-%H%M%S) main`.\n   \
            Nela, `git merge --no-ff agente/<id>` uma por vez, em ordem de dependência (a que outra\n   \
            espera vem antes; sem dependência clara, a ordem da lista). Resolva os conflitos e\n   \
            corrija **só** o que nasce da junção, com commit. Se uma não entra sem mexer no que é\n   \
            dela, `git merge --abort` e devolva essa como no passo 2.\n\
         4. Na mesma worktree, para cada tarefa devolvida que tem item no `docs/FUNCIONALIDADES.md`\n   \
            (ou no backlog em markdown do projeto), acrescente ao item a mesma nota \"Revisão (data):\n   \
            …\". Defeito que não é de tarefa nenhuma vira item novo lá (e card, com `kanban criar`).\n   \
            Commite.\n\
         5. Rode as verificações do projeto na worktree (ex.: `./dev just check`; se o projeto pede\n   \
            que rodem no PC, `\"$ASSISTENTE_FORA_DO_JAIL\" --pc --timeout 1500 './dev just check'`).\n   \
            Vermelho por causa da junção: corrija e rode de novo. Vermelho por uma tarefa só: tire-a\n   \
            (refaça a worktree sem ela) e devolva como no passo 2. Sem check verde, não junte.\n\
         6. Com o check verde, peça ao hubd para andar com a `main` até o commit da worktree:\n   \
            `\"$ASSISTENTE_FORA_DO_JAIL\" kanban juntar --commit \"$(git rev-parse HEAD)\" --tarefas T1,T2`\n   \
            (as juntadas; vazio se só entram as notas). Ele só anda para a frente (fast-forward se\n   \
            a `main` é o checkout do projeto, senão troca a ref conferindo o valor antigo; nunca\n   \
            push), e as tarefas vão para \"validado\" com o selo \"na main\". Se ele recusar porque a\n   \
            `main` andou, nada mudou: não tente de novo, diga no resumo onde parou.\n\
         7. Apague a worktree de revisão (`git worktree remove --force <pasta>`). O worktree e a\n   \
            branch de cada tarefa juntada saem sozinhos quando esta sessão acaba.\n\
         8. Grave o resumo (o que entrou na `main`, com o commit; o que voltou e por quê; itens\n   \
            novos; e, se parou, onde e por quê, com o fim da saída do check):\n   \
            `\"$ASSISTENTE_FORA_DO_JAIL\" kanban resumo --texto \"<resumo em markdown>\"`. Ele aparece\n   \
            no quadro e fica em `.assistente/revisoes/`, fora do git. Repita o resumo aqui no fim.\n\n\
         ## Regras\n\n\
         - A `main` só anda pelo `kanban juntar`. Não faça push, não troque a branch da raiz e não\n  \
           commite nela nem nos worktrees das tarefas.\n\
         - Não mexa em `.assistente/kanban.json`: o quadro muda pelos comandos acima.\n\
         - Check vermelho ou `main` movida: a `main` não muda, e o resumo diz onde parou.\n\
         - Tarefa que você não conseguir avaliar, deixe como está e diga no resumo.\n\
         {extra}",
        n = cards.len(),
        extra = match instructions.trim() {
            "" => String::new(),
            text => format!("\n## Instruções do projeto\n\n{text}\n"),
        },
    )
}

/// As tarefas de um prompt de QA ou de revisão: título, branch, worktree, commit e pedido.
fn card_list(cards: &[Card]) -> String {
    cards
        .iter()
        .map(|card| {
            let description = if card.description.is_empty() {
                "(sem descrição: o título é o pedido)".to_owned()
            } else {
                card.description.clone()
            };
            let commit = card
                .commit
                .as_deref()
                .map(|commit| format!(", marcada como feita no commit `{commit}`"))
                .unwrap_or_default();
            format!(
                "### {id}: {title}\n\n\
                 Branch `{branch}`, worktree `.assistente/worktrees/{id}`{commit}.\n\n\
                 {description}\n\n",
                id = card.id,
                title = card.title,
                branch = branch(&card.id),
            )
        })
        .collect()
}

fn prompt(card: &Card, branch: &str, instructions: &str, marking: &str) -> String {
    let description = if card.description.is_empty() {
        "(sem descrição: o título é o pedido)".to_owned()
    } else {
        card.description.clone()
    };
    format!(
        "# Tarefa {id}: {title}\n\n\
         {description}\n\n\
         ## Regras\n\n\
         - Você está num worktree próprio (branch `{branch}`), separado do que o usuário edita.\n\
         - Trabalhe só aqui. Faça commits pequenos, com mensagem clara.\n\
         - Não faça push, não troque de branch e não mexa em outros branches.\n\
         - **Não rode `./dev`** (o check fica para o QA). Ao terminar, na nota do `done` escreva\n   \
           que a verificação do projeto (`./dev just check`) será feita pelo QA no PC.\n\
         - Ao terminar, deixe a árvore limpa: tudo commitado. Se não der para concluir, deixe\n  \
           o que avançou commitado e explique no fim da resposta o que falta.\n\
         {marking}{extra}",
        id = card.id,
        title = card.title,
        extra = match instructions.trim() {
            "" => String::new(),
            text => format!("\n## Instruções do projeto\n\n{text}\n"),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kanban::{Column, Kind, Priority};

    fn card(description: &str) -> Card {
        Card {
            id: "T7".to_owned(),
            title: "Tela de login".to_owned(),
            description: description.to_owned(),
            priority: Priority::Normal,
            kind: Kind::Agente,
            column: Column::Fazendo,
            holder: None,
            commit: None,
            failures: 0,
            blocked: None,
            last_error: None,
            merged: None,
            depends_on: Vec::new(),
            created_at: 1,
        }
    }

    #[test]
    fn model_names_stay_inside_the_safe_alphabet() {
        assert!(validate_model("opencode/big-pickle").is_ok());
        assert!(validate_model("provedor:modelo-free.v2").is_ok());
        assert!(validate_model("").is_err());
        assert!(validate_model("a b").is_err());
        assert!(validate_model("a;rm").is_err());
        assert!(validate_model(&"m".repeat(MAX_MODEL_LEN + 1)).is_err());
    }

    #[test]
    fn models_are_deduplicated_and_bounded() {
        let picked = checked_models(&["a".into(), "b".into(), "a".into()]).unwrap();
        assert_eq!(picked, vec!["a".to_owned(), "b".to_owned()]);
        assert!(checked_models(&[]).is_err());
        let many: Vec<String> = (0..=MAX_MODELS).map(|n| format!("m{n}")).collect();
        assert!(checked_models(&many).is_err());
    }

    #[test]
    fn task_ids_are_only_the_generated_shape() {
        assert!(valid_task_id("T1"));
        assert!(valid_task_id("T120"));
        assert!(!valid_task_id("T"));
        assert!(!valid_task_id("../x"));
        assert!(!valid_task_id("T1/../../x"));
        assert!(!valid_task_id("t1"));
        assert!(!valid_task_id("T1 2"));
    }

    #[test]
    fn worktree_and_branch_follow_the_task() {
        let dir = worktree_dir(Path::new("/p/projeto"), "T7");
        assert_eq!(dir, PathBuf::from("/p/projeto/.assistente/worktrees/T7"));
        assert_eq!(branch("T7"), "agente/T7");
    }

    #[test]
    fn files_live_under_the_profile_data_dir() {
        let files = files(Path::new("/d/perfil"), "pessoal--proj--opencode-1");
        assert_eq!(
            files.log,
            PathBuf::from("/d/perfil/trabalhadores/pessoal--proj--opencode-1.log")
        );
        assert!(files.prompt.ends_with("pessoal--proj--opencode-1.md"));
        assert!(files.run.ends_with("pessoal--proj--opencode-1.json"));
        assert!(
            files
                .outcomes
                .ends_with("pessoal--proj--opencode-1.modelos")
        );
    }

    #[test]
    fn free_models_are_the_zero_cost_models_of_the_opencode_provider() {
        let catalogue = r#"{
            "opencode": {"models": {
                "big-pickle": {"cost": {"input": 0, "output": 0}},
                "glm-5-free": {"cost": {"input": 0, "output": 0}, "status": "deprecated"},
                "kimi-k2": {"cost": {"input": 0.4, "output": 2.5}},
                "sem-preco": {"name": "x"},
                "so-entrada": {"cost": {"input": 0, "output": 1}}
            }},
            "9router": {"models": {"free": {"cost": {"input": 0, "output": 0}}}},
            "openrouter": {"models": {"x:free": {"cost": {"input": 0, "output": 0}}}}
        }"#;
        assert_eq!(
            free_models(catalogue),
            vec!["opencode/big-pickle".to_owned()]
        );
        assert!(free_models("não é json").is_empty());
        assert!(free_models("{}").is_empty());
        assert_eq!(
            catalogue_path(Path::new("/home/rafa")),
            PathBuf::from("/home/rafa/.cache/opencode/models.json")
        );
    }

    #[test]
    fn outcomes_are_counted_per_model_and_bad_lines_are_skipped() {
        let outcomes =
            parse_outcomes("a ok\na falhou\na ok\nb falhou\nlixo\nc ok extra\nd talvez\n");
        assert_eq!(outcomes.get("a"), Some(&(2, 1)));
        assert_eq!(outcomes.get("b"), Some(&(0, 1)));
        assert!(!outcomes.contains_key("c"));
        assert!(!outcomes.contains_key("d"));
    }

    #[test]
    fn ranking_prefers_success_and_keeps_discovery_order_on_ties() {
        let candidates: Vec<String> = ["x/novo", "x/ruim", "x/bom"].map(String::from).to_vec();
        let outcomes = parse_outcomes("x/bom ok\nx/bom ok\nx/ruim falhou\nx/ruim falhou\n");
        assert_eq!(
            rank_models(&candidates, &outcomes),
            vec!["x/bom".to_owned(), "x/novo".to_owned(), "x/ruim".to_owned()]
        );
        assert_eq!(rank_models(&candidates, &Outcomes::new()), candidates);
    }

    #[test]
    fn read_outcomes_sums_every_run_of_the_profile() {
        let dir = std::env::temp_dir().join(format!("hub-core-modelos-{}", std::process::id()));
        let trab = dir.join("trabalhadores");
        std::fs::create_dir_all(&trab).unwrap();
        std::fs::write(trab.join("s1.modelos"), "a ok\na falhou\n").unwrap();
        std::fs::write(trab.join("s2.modelos"), "a ok\nb falhou\n").unwrap();
        std::fs::write(trab.join("s2.log"), "a ok\n").unwrap();
        let total = read_outcomes(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(total.get("a"), Some(&(2, 1)));
        assert_eq!(total.get("b"), Some(&(0, 1)));
        assert!(read_outcomes(&dir).is_empty());
    }

    #[test]
    fn prompt_carries_the_task_and_the_rules() {
        let text = render_prompt(&card("Pedir e-mail e senha."), "agente/T7", "");
        assert!(text.starts_with("# Tarefa T7: Tela de login"));
        assert!(text.contains("Pedir e-mail e senha."));
        assert!(text.contains("branch `agente/T7`"));
        assert!(text.contains("Não faça push"));
        // Trabalhador não roda ./dev; o check fica para o QA no PC
        assert!(text.contains("Não rode `./dev`"));
        assert!(text.contains("check fica para o QA"));
        assert!(!text.contains("Instruções do projeto"));
        let empty = render_prompt(&card(""), "agente/T7", "");
        assert!(empty.contains("sem descrição"));
        let extra = render_prompt(&card("x"), "agente/T7", "  Use pnpm, nunca npm.\n");
        assert!(
            extra.ends_with("\n## Instruções do projeto\n\nUse pnpm, nunca npm.\n"),
            "{extra}"
        );
    }

    #[test]
    fn finish_prompt_continues_the_worktree_and_marks_the_card_itself() {
        let mut stopped = card("Pedir e-mail e senha.");
        stopped.blocked = Some("Free usage limit reached.".to_owned());
        let text = render_finish_prompt(&stopped, "agente/T7", "Use pnpm.");
        assert!(text.starts_with("# Tarefa T7: Tela de login"));
        assert!(text.contains("parou (motivo: Free usage limit reached.)"));
        assert!(text.contains("kanban done T7 --commit"));
        assert!(text.contains("kanban release T7"));
        assert!(!text.contains("quem marca é o"));
        assert!(text.ends_with("Use pnpm.\n"));
        let plain = render_finish_prompt(&card("x"), "agente/T7", "");
        assert!(plain.contains("já começou esta tarefa aqui e parou. Antes"));
    }

    #[test]
    fn qa_prompt_asks_where_to_merge_and_validates_or_reopens() {
        let mut done = card("Pedir e-mail e senha.");
        done.commit = Some("abc1234".to_owned());
        let text = render_qa_prompt(&done, "agente/T7", "");
        assert!(text.starts_with("# QA da tarefa T7: Tela de login"));
        assert!(text.contains("Pedir e-mail e senha."));
        assert!(text.contains("na `main` ou em outra branch"));
        assert!(text.contains("commit `abc1234`"));
        assert!(text.contains("kanban reabrir T7 --nota"));
        assert!(text.contains("kanban validar T7 --branch ALVO"));
        // O validar roda o check no PC (F11.17): vermelho, nada junta
        assert!(text.contains("o check do projeto no PC"));
        assert!(text.contains("Vermelho: nada muda"));
        assert!(!text.contains("Instruções do projeto"));
        let extra = render_qa_prompt(&card("x"), "agente/T7", "Use pnpm.");
        assert!(
            extra.ends_with("\n## Instruções do projeto\n\nUse pnpm.\n"),
            "{extra}"
        );
    }

    #[test]
    fn qa_all_prompt_lists_every_card_and_asks_once() {
        let mut first = card("Pedir e-mail e senha.");
        first.commit = Some("abc1234".to_owned());
        let mut second = card("");
        second.id = "T8".to_owned();
        let text = render_qa_all_prompt(&[first, second], "");
        assert!(text.starts_with("# QA geral do quadro"));
        assert!(text.contains("destas 2 tarefas"));
        assert!(text.contains("### T7: Tela de login"));
        assert!(text.contains("Branch `agente/T7`, worktree `.assistente/worktrees/T7`, marcada"));
        assert!(text.contains("### T8: Tela de login\n\nBranch `agente/T8`"));
        assert!(text.contains("sem descrição"));
        assert!(text.contains("Uma resposta vale para todas"));
        assert!(text.contains("kanban validar <id> --branch ALVO"));
        // O validar roda o check no PC (F11.17): vermelho, nada junta
        assert!(text.contains("o check do projeto no PC"));
        assert!(text.contains("Vermelho: nada muda"));
        assert!(!text.contains("Instruções do projeto"));
    }

    #[test]
    fn review_prompt_merges_into_main_through_the_hub_and_never_asks() {
        let mut first = card("Pedir e-mail e senha.");
        first.commit = Some("abc1234".to_owned());
        let mut second = card("");
        second.id = "T8".to_owned();
        let text = render_review_prompt(&[first, second], "");
        assert!(text.starts_with("# Revisão do quadro"));
        assert!(text.contains("estas 2 tarefas"));
        assert!(text.contains("### T7: Tela de login"));
        assert!(text.contains("### T8: Tela de login\n\nBranch `agente/T8`"));
        assert!(text.contains("não pergunte nada"));
        assert!(text.contains("git diff main...agente/<id>"));
        assert!(text.contains("worktree add --detach .assistente/worktrees/revisao-"));
        assert!(text.contains("ordem de dependência"));
        assert!(text.contains("kanban reabrir <id> --nota"));
        assert!(text.contains("docs/FUNCIONALIDADES.md"));
        assert!(text.contains("./dev just check"));
        assert!(text.contains("kanban juntar --commit"));
        assert!(text.contains("kanban resumo --texto"));
        assert!(!text.contains("kanban validar"));
        assert!(!text.contains("Instruções do projeto"));
        let extra = render_review_prompt(&[card("x")], "Use pnpm.");
        assert!(extra.ends_with("\n## Instruções do projeto\n\nUse pnpm.\n"));
    }

    #[test]
    fn config_defaults_and_parse() {
        let config: WorkersConfig = toml::from_str("models = [\"x/y\"]").unwrap();
        assert_eq!(config.models, vec!["x/y".to_owned()]);
        assert_eq!(config.attempts, 3);
        assert!(toml::from_str::<WorkersConfig>("modelo = \"x\"").is_err());
    }
}
