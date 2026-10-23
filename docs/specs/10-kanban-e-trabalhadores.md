# 10 — Quadro, orquestrador e trabalhadores

Cobre o quadro por projeto (F11). Funcionalidades F11.1–F11.16. O que e por que: `docs/VISAO.md`
(R6 modo dormir, R7 sessões). Este documento é o como.

## Quadro

`.assistente/kanban.json` no repo do projeto é a fonte da verdade. O hubd lê e grava o arquivo e
comita **só ele** a cada escrita (`git commit -- .assistente/kanban.json`): o que você já tinha no
index fica como estava, e a árvore não amanhece suja (o modo dormir exige árvore limpa, F6.3).

Colunas: `backlog` · `fazendo` · `feito` · `validado` (F11.15). Campos de um card: `id` (`T1`, nunca
reaproveitado), `title`, `description`, `priority` (`alta` · `normal` · `baixa`), `kind`, `column`,
`holder` (a sessão que segura: o trabalhador em "fazendo", o QA em "feito"/"validado"), `commit` (o
que fechou a tarefa, em "feito"), `merged` `{branch, commit}` (onde o QA juntou, em "validado"), `failures` (vezes que voltou do
"fazendo" sem concluir: `release` ou sessão morta; omitido quando zero), `blocked` (a linha de erro
de quando parou por cota, F11.14; some quando a tarefa começa de novo), `depends_on` (ids `T<n>`
das tarefas das quais esta depende, F11.13; omitido quando vazio). No nível do quadro,
`orchestrator` (`enabled`, `max_workers`, `instructions`, `paused` `{reason, at}`), omitido enquanto
estiver no padrão (desligado, 1, sem instruções, sem pausa).

**Tipos de card** (F11.6):

- `manual` (padrão): nunca roda sozinho. Só roda quando você manda ("Mandar" no card).
- `agente`: marcado para o agente implementar. O orquestrador (F11.7) e o modo dormir (F11.4)
  puxam a próxima tarefa `agente` do backlog: prioridade, depois criação, depois número.

**Dependências** (F11.13): nenhum trabalhador abre (`pull`, orquestrador automático, "Mandar") para
card cuja dependência ainda não está na `main`, isto é, validada pelo QA com o commit de `merged` no
`HEAD` do projeto (`git merge-base --is-ancestor`). O worktree nasce do `HEAD`, então já sai com o
código dela. "Feito" não basta: o commit só está na branch `agente/<id>`. Dependência que não existe
nunca é atendida. O "Popular pelo backlog" preenche pelo "Ordem:" das seções e pelo "Depende de" dos
itens; a engrenagem do card edita.

**Posse**: só quem segura a tarefa conclui (`done`) ou solta (`release`). Duas sessões nunca levam a
mesma tarefa (o pull é atômico sob `kanban_lock`). Sessão morta = tarefa solta na próxima leitura do
quadro, com commit `kanban: solta T3 (sessão encerrada)`. Regras puras em `hub_core::kanban`.

## Popular pelo backlog (F11.9)

**Prompt inicial em arquivo.** Toda sessão do Claude que já começa com tarefa (esta, o "Terminar com
Claude" e os QAs) recebe o prompt em `.assistente/prompts/<sessão>.md`, na pasta dela (projeto ou
worktree, fora do `git status` pelo `info/exclude`). O argumento do `claude` só aponta para o
arquivo. O prompt inteiro no comando fazia o tmux recusar a sessão ("command too long") no QA geral
com vários cards. Na pasta da sessão, o Claude lê sem pedir permissão.

`POST …/kanban/explorar` abre um Claude no projeto cujo prompt (`hub_core::worker::EXPLORE_PROMPT`) é
gravado em `~/.assistente/perfis/<perfil>/explorar/<sessão>.md`. O Claude lê o backlog em markdown,
usa `kanban listar` para não duplicar e `kanban criar` (`POST /v1/kanban/criar`, token da sessão) para
cada item pendente. Cards nascem `manual`. A sessão não implementa nada.

## Trabalhador

Uma sessão do opencode no jail, rodando `vm/trabalhador`, no worktree da tarefa:

- Progresso conta só com mudança própria (F11.13): um commit da linha da tarefa (primeiro pai), que
  não seja merge e mude algum arquivo. Só um merge (o diff dele é o da outra branch) é desfeito e
  não fecha o card.
- Worktree: `<projeto>/.assistente/worktrees/<id>`, branch `agente/<id>`. Fica fora do `git status`
  pelo `info/exclude`. Você revisa e mescla a branch (F10.4 mostra o diff).
- Prompt e registro: `~/.assistente/perfis/<perfil>/trabalhadores/<sessão>.{md,json}`, fora do repo
  e dentro do mount de dados do perfil (o jail enxerga). O `.log` é a saída do loop; `.modelos` tem
  um `ok` ou `falhou` por tentativa (F11.10).
- `[workers]` do `.assistente.toml`: `models` (principal primeiro, depois reservas), `attempts`
  (tentativas por modelo), `max_minutes` e `wait_secs`. `models` é opcional (F11.10).

O opencode do instalador (`~/.opencode/bin`, fora do PATH) vai para o jail só de leitura (`--map`) e o
trabalhador recebe o caminho absoluto em `AGENTE_OPENCODE`. Sem isso o trabalhador falha com
`opencode: not found` em todas as tentativas e o card fica parado em "fazendo" (corrigido junto com
F11.10, achado por reprodução com o hubd de verdade).

Escolha dos modelos (F11.10), na ordem: pedido (`--modelos` ou o "Mandar" com modelos) > `[workers]
models` do projeto > automática. A automática lê o catálogo do opencode (`~/.cache/opencode/models.json`,
o mesmo do `/models` da TUI, atualizado pelo próprio opencode a cada chamada), fica com os modelos do
provedor `opencode` de custo zero (entrada e saída), sem os `"status": "deprecated"` (o `opencode
run` recusa esses), e ordena pela taxa de
sucesso do perfil: `(ok+1)/(ok+falhas+2)`, somando os `.modelos` de `trabalhadores/`. Sem histórico,
todos ficam no meio e vale a ordem da lista; até `MAX_MODELS` (5). Não há critério por tipo de tarefa:
o card não guarda categoria, então o histórico é por modelo. Nenhum grátis = erro com o que fazer.

Loop (`vm/trabalhador`): para cada modelo, `attempts` tentativas. Progresso = commit novo **e** árvore
limpa. Com progresso: `fora-do-jail kanban done <id> --commit <sha>` e sai com 0. Esgotou os modelos:
`fora-do-jail kanban release <id>` e sai com 1. O loop não conhece o quadro: fala com o hubd pelo
token da própria sessão. Testado por `vm/teste-trabalhador.sh` (opencode e fora-do-jail falsos).
O `opencode run` vai com `--standalone`, como o agente: sem ele tenta o servidor compartilhado na
porta 49374 (a do ai-memory) e aborta toda tentativa com "Managed service port 49374 … in use".
A saída do `opencode run` vai para a tela da sessão (`tee`) e para o `.log`: a aba do trabalhador
mostra o agente trabalhando, não só as linhas do loop.

**Cota esgotada** (F11.14): a saída do opencode com `ERRO_COTA` ("Free usage limit reached",
"Subscribe to OpenCode Go"; visto na VM em 2026-10-22) não é falha do modelo. Os grátis dividem a
mesma cota, então trocar de modelo só queimaria os outros e, com o orquestrador ligado, o quadro
inteiro. O `trabalhador` mata a tentativa (126), não grava resultado no histórico, roda
`fora-do-jail kanban bloquear <id> --motivo <linha do erro>` e sai com 3. O hubd (`POST
/v1/kanban/block`) devolve o card ao backlog **sem** `failures + 1`, com `blocked`, e pausa o
orquestrador do projeto (`paused`, a hora da primeira recusa). Trabalhadores que já rodam seguem; se
baterem na cota, bloqueiam do mesmo jeito.

Vigia (F11.10): cada tentativa roda o opencode num grupo próprio e olha a saída a cada
`AGENTE_VIGIA` segundos (padrão 5). Erro de provedor (`rate limit`, `429`/`5xx`, `model not found`, `model unavailable`, `has been deprecated`, `upstream request failed`,
`ECONNRESET`, …) mata o grupo e troca de modelo sem esperar o resto da tentativa (código 125). A
lista de padrões fica em `ERRO_PROVEDOR` no script; o risco é casar texto que o agente imprimiu por
acaso, então a lista é estreita de propósito. Para as mensagens que a lista não conhece, vale o tempo:
o opencode que sai com erro em menos de `AGENTE_RAPIDO` segundos (padrão 60; 0 desliga) nem chegou a
trabalhar, e também troca de modelo (os grátis falham de jeitos variados: `Endpoint is unavailable`,
`Model unavailable`, `deprecated`).

## Orquestrador

A sessão do Claude. Pelo `fora-do-jail` (instruções em `vm/configurar-agentes.sh`):

1. `kanban pull`: pega uma tarefa `agente` (ou `{"card": null}`: parar).
2. `kanban trabalhador <id> --modelos a,b,c`: `POST /v1/workers`. Só sessão do Claude abre
   trabalhador; trabalhador não abre outro. A tarefa passa de dono (`hand_over`).

Pelo `fora-do-jail` o pedido **não** passa por aprovação no app: não roda nada no PC nem na VM fora do
jail; é coordenação entre sessões.

## Orquestrador automático (F11.11)

Um laço do próprio hubd (`spawn_orchestrator`, iniciado no `main`), sem sessão do Claude e sem
gastar tokens: o que o orquestrador Claude fazia (puxar e abrir trabalhador) agora não precisa de
decisão, porque o modelo já é escolhido sozinho (F11.10).

- Liga por projeto no quadro: botão "Orquestrador" + limite (1–4, `MAX_WORKERS`). `PUT
  …/kanban/orquestrador {enabled, max_workers}` grava em `kanban.json` com commit (`kanban:
  orquestrador ligado (até 2 em fazendo)`) e já dá a primeira volta antes de responder.
- A cada 20 s (`ORCHESTRATOR_EVERY`), em cada projeto ligado: enquanto as tarefas em "fazendo" (de
  qualquer origem, Mandar incluso) estiverem abaixo do limite, abre um trabalhador para a próxima
  tarefa `agente` do backlog (`Board::next_auto`, mesma ordem do `pull`), pelo mesmo caminho do
  "Mandar" (`start_worker`: worktree, modelos, posse).
- **Não insiste**: `next_auto` pula cards com `failures > 0`. O trabalhador que esgotou os modelos,
  o "Parar" da UI e a sessão que morreu (inclusive VM reiniciada) devolvem o card com `failures + 1`,
  e ele fica no backlog com "Voltou N× sem concluir". "Tentar de novo" (`PATCH {failures: 0}`) ou
  "Mandar" põem de volta. O `pull` explícito (orquestrador Claude, `dorme`) não olha `failures`.
- **Instruções dos trabalhadores** (`orchestrator.instructions`, botão ao lado do limite): texto
  livre que `worker::render_prompt` põe no fim do prompt, como "## Instruções do projeto", depois
  das regras padrão. Vale para todo trabalhador do projeto (automático, Mandar, Reiniciar). `PUT`
  sem `instructions` mantém as atuais.
- **Aviso**: a resposta do `PUT` traz `started` (as tarefas que a primeira volta abriu) e a UI mostra
  um toast com o resultado: o que foi para trabalhador, o erro, ou que não havia card livre.
  Enquanto o `PUT` roda, o botão mostra "Ligando…"/"Desligando…".
- Uma volta por vez (`orchestrator_lock`): o laço e o "ligar" da UI não disputam a mesma tarefa.
- **Pausa por cota** (F11.14): com `paused`, a volta não abre nada (e não é erro). O quadro mostra
  uma faixa ("Cota do provedor esgotada às HH:MM: orquestrador pausado"), com o motivo, e um toast
  quando a pausa aparece com o quadro aberto. "Retomar" (`POST …/kanban/orquestrador/retomar`) tira
  a pausa e já dá uma volta (`started`, como ao ligar); os cards bloqueados voltam a ser puxados.
  O `PUT …/orquestrador` (ligar, limite, instruções) não mexe na pausa.
- **Terminar com outro harness** (F11.14): o card com `blocked` mostra o selo "cota"; ele e o card
  que voltou sem concluir (`failures > 0`) mostram dois botões.
  "Terminar com Claude" (`POST …/kanban/{id}/claude`) abre um Claude no worktree da tarefa
  (`LaunchPlan::build_finish`: `.git` do projeto gravável, API dos agentes) com
  `worker::render_finish_prompt`: a mesma tarefa, "continue de onde o trabalhador parou" e, como
  não há trabalhador, ele mesmo roda `kanban done` (ou `release`). A sessão segura o card. "Outro
  modelo…" é o "Mandar" com um modelo escolhido (padrão `9router/free`). Os dois valem mesmo com o
  orquestrador pausado: são pedidos seus.
- Erro (sem API dos agentes, sem modelo grátis…) não para o laço: fica em memória por projeto, vai
  no `GET …/kanban` como `orchestrator_error` (faixa amarela no quadro) e no log só quando muda.

## QA e validado (F11.15)

O botão **QA** no card "feito" (sem ninguém segurando) abre um Claude numa aba (`POST
…/kanban/{id}/qa`), no worktree da tarefa (`LaunchPlan::build_finish`, o mesmo do "Terminar com
Claude"), com `worker::render_qa_prompt`. A sessão segura o card (`Board::start_qa`): um QA por vez.
O Claude:

1. Pergunta a você onde juntar: `main` ou outra branch. Espera a resposta.
2. Lê o diff da branch `agente/<id>` contra a escolhida, confere com o pedido do card e roda as
   verificações do projeto. Não implementa o que falta.
3. Não atende: `fora-do-jail kanban reabrir <id> --nota "…"` (`POST /v1/kanban/reopen`). O card volta
   ao backlog com "Revisão (AAAA-MM-DD): nota" no fim da descrição, `failures` zerado e sem
   `commit`: o automático puxa de novo e o próximo trabalhador continua da mesma branch, que o
   worktree mantém.
4. Atende: `fora-do-jail kanban validar <id> --branch B` (`POST /v1/kanban/validate`). O hubd junta
   (`merge_task`), só para a frente e sem push: a branch já contém a tarefa = nada muda; é
   ancestral = fast-forward; senão, `merge --no-ff` numa worktree descartável, e conflito = 409 sem
   mexer em nada (o QA roda `git merge B` no worktree, resolve, commita e valida de novo). Branch
   que não existe nasce na ponta da tarefa. Quem tem `B` em checkout (a pasta do projeto, em geral
   a `main`) anda com `merge --ff-only`, que recusa se encostar em mudança sua; senão a ref troca
   conferindo o valor antigo (como a F8.8). Árvore suja no worktree = 409: o que não está commitado
   não entraria. O card vai para "validado" com `merged`.

Só quem segura valida ou reabre. Quando a sessão do QA acaba, o card perde o `holder` sem `failures`
(fora de "fazendo", `release_dead` só solta) e, se está validado, o worktree sai (`git worktree
remove`, que recusa com mudança) e a branch `agente/<id>` também, se já está no commit juntado.
O card validado mostra "na main · abc1234". O contador da lateral conta só backlog e fazendo.

**QA geral** (F11.16), para limpar o quadro: o botão "QA geral (N)" no topo da coluna Feito (`POST
…/kanban/qa`) abre um Claude só, na raiz do projeto (`launch_session`, como o "Popular pelo
backlog"), com `worker::render_qa_all_prompt`: os N cards feitos sem dono, em ordem de id, cada um
com branch e worktree. A sessão segura todos. Ele pergunta **uma vez** onde juntar e passa card por
card com os mesmos `validar`/`reabrir`; o conflito que nasce da junção das anteriores ele resolve no
worktree da tarefa. Card que ele não tocar fica livre quando a sessão acaba. Card feito sem branch
(modo dormir, ou feito à mão) é validado pelo commit marcado: se já está na branch escolhida, só
muda de coluna.

## Supervisão

- Dentro: o `trabalhador` troca de modelo e tenta de novo sozinho (acima).
- Fora: sessão morta solta a tarefa (automático, na leitura). **Reiniciar** (`POST
  /api/sessions/{id}/reiniciar`) mata a sessão, abre outra com o mesmo registro e no mesmo worktree, e
  passa a tarefa para ela. O reinício não apaga o WIP: o worktree é reaproveitado.
- O card mostra quem segura ("em Claude 1") e a contagem de pendências aparece no projeto da lateral.

## Modo dormir no quadro (F11.4)

Opt-in por projeto: `[sleep] source = "quadro"`. O padrão `backlog` não muda nada.

- O hubd usa `quadro_prompt.md` (contrato sem a escolha pelo backlog) e passa `AGENTE_QUADRO`.
- O `dorme` faz, por iteração: `pull` (commit `kanban: T3 puxada por dorme`), roda o agente com a
  tarefa anexada ao prompt, e fecha: commit novo → `done` com o commit; sem commit → `release`
  (volta ao backlog, o log explica). Quadro sem tarefa para agente = para sem iterar.
- Os commits do quadro vão junto no clone do notebook e voltam no merge da manhã (F6.5).

## Testes

- `hub_core::kanban` (regras), `hub_core::worker` (config, prompt, nomes), `session::worker_runs…`.
- `hubd::api::kanban::tests`: CRUD com um commit por escrita, pull/done/release com sessões de tmux de
  verdade, sessão morta, Mandar com worktree e reinício, orquestrador → trabalhador, orquestrador
  automático (limite, card que voltou fica, "Tentar de novo", erro no quadro), cota (bloqueio pausa,
  "Terminar com Claude", "Retomar"), QA (um por vez, só quem segura valida, árvore suja, merge na
  `main` em checkout, conflito sem mudar nada, branch nova, reabrir com nota, worktree e branch saem
  quando o QA acaba), QA geral (segura só os feitos livres, valida pelo commit quando não há
  branch, solta o resto quando acaba).
- `vm/teste-trabalhador.sh` (inclui cota: na saída final e no meio da tentativa) e
  `notebook/teste-dorme.sh` (quadro: prioridade, commit, volta ao backlog, quadro vazio).
- `KanbanBoard.test.tsx`, `ProjectList.test.tsx`.
- Não coberto aqui (F11.8, humano): opencode real, nomes de modelo do `opencode models`.
