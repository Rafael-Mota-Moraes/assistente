# 10 — Quadro, orquestrador e trabalhadores

Cobre o quadro por projeto (F11). Funcionalidades F11.1–F11.8. O que e por que: `docs/VISAO.md`
(R6 modo dormir, R7 sessões). Este documento é o como.

## Quadro

`.assistente/kanban.json` no repo do projeto é a fonte da verdade. O hubd lê e grava o arquivo e
comita **só ele** a cada escrita (`git commit -- .assistente/kanban.json`): o que você já tinha no
index fica como estava, e a árvore não amanhece suja (o modo dormir exige árvore limpa, F6.3).

Colunas: `backlog` · `fazendo` · `feito`. Campos de um card: `id` (`T1`, nunca reaproveitado),
`title`, `description`, `priority` (`alta` · `normal` · `baixa`), `kind`, `column`, `holder` (a
sessão que segura, só em "fazendo"), `commit` (só em "feito").

**Tipos de card** (F11.6):

- `manual` (padrão): nunca roda sozinho. Só roda quando você manda ("Mandar" no card).
- `agente`: marcado para o agente implementar. O orquestrador (F11.7) e o modo dormir (F11.4)
  puxam a próxima tarefa `agente` do backlog: prioridade, depois criação, depois número.

**Posse**: só quem segura a tarefa conclui (`done`) ou solta (`release`). Duas sessões nunca levam a
mesma tarefa (o pull é atômico sob `kanban_lock`). Sessão morta = tarefa solta na próxima leitura do
quadro, com commit `kanban: solta T3 (sessão encerrada)`. Regras puras em `hub_core::kanban`.

## Popular pelo backlog (F11.9)

`POST …/kanban/explorar` abre um Claude no projeto cujo prompt (`hub_core::worker::EXPLORE_PROMPT`) é
gravado em `~/.assistente/perfis/<perfil>/explorar/<sessão>.md`. O Claude lê o backlog em markdown,
usa `kanban listar` para não duplicar e `kanban criar` (`POST /v1/kanban/criar`, token da sessão) para
cada item pendente. Cards nascem `manual`. A sessão não implementa nada.

## Trabalhador

Uma sessão do opencode no jail, rodando `vm/trabalhador`, no worktree da tarefa:

- Worktree: `<projeto>/.assistente/worktrees/<id>`, branch `agente/<id>`. Fica fora do `git status`
  pelo `info/exclude`. Você revisa e mescla a branch (F10.4 mostra o diff).
- Prompt e registro: `~/.assistente/perfis/<perfil>/trabalhadores/<sessão>.{md,json}`, fora do repo
  e dentro do mount de dados do perfil (o jail enxerga). O `.log` é a saída do loop.
- `[workers]` do `.assistente.toml`: `models` (principal primeiro, depois reservas), `attempts`
  (tentativas por modelo), `max_minutes` e `wait_secs`. Sem `models` o "Mandar" pede o modelo.

Loop (`vm/trabalhador`): para cada modelo, `attempts` tentativas. Progresso = commit novo **e** árvore
limpa. Com progresso: `fora-do-jail kanban done <id> --commit <sha>` e sai com 0. Esgotou os modelos:
`fora-do-jail kanban release <id>` e sai com 1. O loop não conhece o quadro: fala com o hubd pelo
token da própria sessão. Testado por `vm/teste-trabalhador.sh` (opencode e fora-do-jail falsos).

## Orquestrador

A sessão do Claude. Pelo `fora-do-jail` (instruções em `vm/configurar-agentes.sh`):

1. `kanban pull`: pega uma tarefa `agente` (ou `{"card": null}`: parar).
2. `kanban trabalhador <id> --modelos a,b,c`: `POST /v1/workers`. Só sessão do Claude abre
   trabalhador; trabalhador não abre outro. A tarefa passa de dono (`hand_over`).

Pelo `fora-do-jail` o pedido **não** passa por aprovação no app: não roda nada no PC nem na VM fora do
jail; é coordenação entre sessões.

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
  verdade, sessão morta, Mandar com worktree e reinício, orquestrador → trabalhador.
- `vm/teste-trabalhador.sh` e `notebook/teste-dorme.sh` (quadro: prioridade, commit, volta ao backlog,
  quadro vazio).
- `KanbanBoard.test.tsx`, `ProjectList.test.tsx`.
- Não coberto aqui (F11.8, humano): opencode real, nomes de modelo do `opencode models`,
  `--standalone` no `opencode run` dentro do jail.
