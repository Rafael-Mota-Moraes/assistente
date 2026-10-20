# 08 — Modo "hora de dormir"

Cobre R6. Funcionalidades F6.x. Base: o `dorme.sh` do projeto game, que já funciona.

## Ideia

Antes de dormir, um clique entrega o projeto atual ao notebook. Lá, um loop roda o opencode
(dentro do ai-jail, com o 9router e a memória locais) tarefa por tarefa do backlog, sem
humano. O PC pode desligar. De manhã, um clique traz o resultado para revisar e mesclar.

## Fluxo

```
PC (hubd)                                         notebook
─────────                                         ────────
1. confere árvore limpa (ou commit WIP)
2. git push nb:~/assistente/projetos/<perfil>/<proj> \
       HEAD:refs/remotes/pc/<base>  ─────────────► clone (criado na 1ª vez)
3. ssh: git switch -C dorme/<data> pc/<base>  ───► branch de trabalho
4. exporta briefing do ai-memory (servidor PC/VM; réplica no notebook após sync F5.4) → prompt
   (hub_core::sleep::render_prompt)
5. scp prompt → ~/assistente/briefings/<proj>.md
6. ssh: tmux new -d -s dorme-<proj> \
     env AGENTE_PROMPT_FILE=… AGENTE_MAX_MIN=… \
     dorme                                ───────► loop (notebook/dorme)
                                                   └ ai-jail -- opencode run --auto
7. GET /api/sleep: tail do log por ssh             commits em agente/<id>, merge em dorme/<data>
   … PC pode desligar …
8. manhã: POST /api/sleep/collect
   git fetch nb dorme/<data> agente/* ◄──────────── resultado
   → refs/remotes/notebook/*
9. UI: commits por item, RELATORIO.md, log;
   merge fast-forward em <base> com um clique
```

## Config do projeto

`.assistente.toml` → `[sleep]` (exemplo: `config/projeto.exemplo.toml`):

| Chave | Padrão | Uso |
|---|---|---|
| `backlog` | `docs/FUNCIONALIDADES.md` | arquivo com itens `- [ ]` |
| `section` | (todas) | restringe a uma seção |
| `check` | `./dev just check` | precisa passar antes de marcar `[x]` |
| `human_only` | `[]` | trechos de título que o agente nunca escolhe |
| `max_minutes` | 240 | timeout por iteração |
| `attempts` | 3 | iterações sem progresso antes de desistir |
| `model` | (padrão do opencode) | `AGENTE_MODEL` |
| `extra_instructions` | — | regras do projeto no prompt |

## Contrato do agente

O prompt (`crates/hub-core/src/sleep_prompt.md`) é o do game generalizado: uma tarefa por
iteração; branch `agente/<id>`; commits a cada ~30 min; check até 3 tentativas; marcar `[x]`;
merge `--no-ff` em `dorme/<data>`; árvore limpa no fim; RELATORIO.md para parar; nunca push,
rebase, reset --hard, amend ou mexer em `.git`.

## Paradas

O loop (`notebook/dorme`) para quando: existe o arquivo STOP (`POST /api/sleep/stop` ou
`touch ~/assistente/STOP`), o agente escreveu RELATORIO.md, a árvore ficou suja, o agente
terminou fora da base, ou houve `attempts` iterações seguidas sem commit novo.

## Segurança

- O agente noturno roda no ai-jail do notebook: só o clone do projeto é gravável.
- Nenhum push para remotos de verdade: o notebook não tem as suas chaves do GitHub.
- O merge de volta é sempre ação humana, e só fast-forward; se a base andou no PC durante a
  noite, a UI avisa e você mescla à mão.

## Testes (F6.1, F6.2, F6.5)

- `notebook/teste-dorme.sh` (no `just check`): repo temporário, `AGENTE_OPENCODE` falso que faz
  commits ou escreve RELATORIO, conferindo cada condição de parada. `AGENTE_ESPERA=0` tira a
  espera entre tentativas sem progresso.
- Sync: dois repos locais no devenv simulando PC e notebook (`file://`).
