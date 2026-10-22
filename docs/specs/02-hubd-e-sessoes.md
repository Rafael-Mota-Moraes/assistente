# 02 — hubd e sessões

Cobre R7 (Claude e OpenCode) e a base de R1. Funcionalidades F0.3, F1.4, F2.x.

## Processo

`hubd --config ~/.config/assistente/assistente.toml [--listen 127.0.0.1:7878]`

- Roda na VM como o seu usuário (systemd). Em desenvolvimento: `./dev just hubd`.
- Sem token só aceita escutar em loopback; na VM o token é obrigatório.
- Estado: as sessões **são** as sessões do tmux. O hubd não guarda banco: perfil, projeto e
  agente ficam em opções da sessão (`@assistente_profile`, `@assistente_project`,
  `@assistente_agent`) e a lista vem de `tmux list-sessions -F` (sessões sem essas opções
  ficam de fora). Cada sessão também recebe `status off` (as abas do app substituem a barra)
  e `mouse on` (rolagem e clique nos TUIs).
- **Sessão morta continua na tela (F14.3):** cada sessão nasce com `remain-on-exit` **na
  janela dela** (não no servidor tmux do usuário). Quando o processo morre, o painel fica
  com a saída e o status de saída, até alguém encerrar a sessão. A sessão morta não segura
  tarefa do quadro nem token da API dos agentes: para o quadro ela conta como encerrada.
- O servidor tmux nasce como filho do hubd; `KillMode=process` no systemd faz só o hubd
  reiniciar, sem matar as sessões.

## API

Todas as rotas exceto `/api/health` exigem `Authorization: Bearer <token>`. Erros:
`{"error": "...", "feature"?: "F2.2"}`. CORS liberado para `tauri://localhost`,
`http://tauri.localhost` e `http://localhost:1420`.

| Método | Rota | Status | Corpo / resposta |
|---|---|---|---|
| GET | `/api/health` | pronto | `{ok, version, uptime_seconds, commit: {hubd, ui}}` |
| GET | `/ui/*` | pronto (F8.3) | build da UI (`just build-ui`), aberto sem token; rota ausente se a pasta não existe |
| GET | `/api/profiles` | pronto | `[{id, label, color, root, claude_login}]`; `claude_login` = há `.credentials.json` no `CLAUDE_CONFIG_DIR` do perfil (ou `claude_via_9router`) |
| GET | `/api/projects?profile=<id>` | pronto | `[{name, path, has_project_config}]` |
| GET | `/api/projects/{profile}/{project}/settings` | pronto (F9.2) | `{settings: {docker, allow_hosts, full_network, allowed_commands}, docker_from_project_file}` |
| PUT | `/api/projects/{profile}/{project}/settings` | pronto (F9.2) | `settings` → mesma resposta; 400 host/comando inválido; vale para sessões novas |
| GET | `/api/notebook` | parcial (F4.2) | `{ssh_host, tunnel_managed, services: [{name, port, up}]}` |
| GET | `/api/sessions` | pronto | `[{id, profile, project, agent, created_at, attached}]` |
| POST | `/api/sessions` | pronto | `{profile, project, agent: "claude"|"opencode"|"shell"}` → sessão (`shell` = aba Terminal, F9.1) |
| DELETE | `/api/sessions/{id}` | pronto | mata a sessão tmux |
| GET (WS) | `/api/sessions/{id}/terminal` | pronto | ver protocolo abaixo |
| GET | `/api/sleep` | pronto (F6.3) | `?profile&project` → `{sleeping, log_tail}` (tmux + tail do log no notebook) |
| POST | `/api/sleep` | pronto (F6.3) | `{profile, project, wip?}` → 202 `{date, session, remote_dir, prompt_file, log_file}`; 409 árvore suja/já rodando |
| POST | `/api/sleep/stop` | pronto (F6.4) | 204; cria `STOP` no notebook via ssh (502 sem notebook) |
| POST | `/api/sleep/collect` | pronto (F6.5) | `{profile, project, date}` → `{base, sleep_branch, commits, relatorio, fast_forward}`; 502 sem notebook |
| POST | `/api/sleep/merge` | pronto (F6.5) | 204 fast-forward; 409 se a base andou durante a noite |
| GET | `/api/exec` | pronto (F9.7/F9.9) | `[{id, session, profile, project, cwd, command, target, timeout_secs, created_at, state, auto_approved, result}]`: pendentes primeiro, depois os dos últimos 30 min; `target: "host"` é o pedido para o PC (F9.9) |
| POST | `/api/exec/{id}/approve` | pronto (F9.7/F9.9) | `{always?}` → 204, roda na VM fora do jail; pedido `target: host` é só marcado como rodando (quem executa é o app no PC, F9.10) e `always` é recusado (400); `always` grava em `allowed_commands` do projeto; 404/409 |
| POST | `/api/exec/{id}/deny` | pronto (F9.7) | 204; 404/409 se não está pendente |
| GET | `/api/exec/{id}` | pronto (F9.10) | um pedido (o app confere o que foi aprovado antes de rodar no PC); 404 |
| POST | `/api/exec/{id}/result` | pronto (F9.10) | `{exit_code?, stdout?, stderr?, truncated?, timed_out?, duration_ms?, error?}` → 204; o app devolve o que rodou no PC e o agente recebe pelo long-poll; só em pedido `target: host` já aprovado (409 nos outros casos); `error` fecha o pedido como `failed` |
| GET | `/api/memory` | F5.0 / F5.4 / F5.6 | saúde local (F5.0); sync do espelho (F5.4); detalhes operacionais (F5.6) |

## Lançamento de uma sessão

`hub_core::session::LaunchPlan::build(config, home, spec, seq)` monta tudo sem efeitos:

- **nome tmux**: `<perfil>--<projeto>--<agente>-<n>` (minúsculo, sem `.`/`:`);
- **cwd**: `<raiz do perfil>/<projeto>` (nome sem `/`, sem começar com `.`);
- **comando**: `ai-jail --rw-map ~/.assistente/perfis/<perfil> -- claude|opencode`; na aba
  Terminal (`shell`, F9.1), `bash -l` direto, **sem** ai-jail (é você digitando);
- **env**: `GIT_AUTHOR_*`/`GIT_COMMITTER_*`, `CLAUDE_CONFIG_DIR`, `OPENCODE_CONFIG`,
  `AI_MEMORY_SERVER_URL`, `ANTHROPIC_BASE_URL` (só com `claude_via_9router`),
  `GIT_SSH_COMMAND` (só com `ssh_key`) e `ASSISTENTE_PROFILE`; com a API dos agentes (F9.5),
  `ASSISTENTE_EXEC_URL`, `ASSISTENTE_EXEC_TOKEN`, `ASSISTENTE_FORA_DO_JAIL` e, no Claude,
  `BASH_MAX_TIMEOUT_MS`.

`LaunchPlan::tmux_new_session_argv()` vira `tmux new-session -d -s … -c … -e K=V … -- <comando>`.

## API dos agentes (F9.5)

Segundo listener, `[hub] agent_listen` (padrão `127.0.0.1:7879`, só loopback). É a **única**
porta do hubd que entra no ai-jail (`--forward-port`), com um roteador à parte (`agent_router`):
nada de `/api/*`, nem saúde, UI ou aprovação. Sem o bind (porta ocupada) o hubd sobe igual e as
sessões nascem sem a API.

- **Token por sessão.** Ao criar uma sessão de agente (não a aba Terminal), o hubd sorteia 32
  bytes (`/dev/urandom`), guarda na opção `@assistente_exec_token` do tmux e entrega ao jail
  por `--env ASSISTENTE_EXEC_TOKEN` (com `ASSISTENTE_EXEC_URL` e `ASSISTENTE_FORA_DO_JAIL`).
  Nunca vai para o `SessionInfo`. Sessão encerrada = token inválido.
- **Pré-requisito:** o cliente em `~/.local/lib/assistente/fora-do-jail` (instalado pelo
  `provisionar.sh`, entra no jail só leitura); sem ele, a sessão nasce sem a API.

| Método | Rota | Corpo / resposta |
|---|---|---|
| POST | `/v1/exec` | `{command, cwd?, timeout_secs?, target?}` → 202 `{id, state, auto_approved}`; `target: "host"` = pedido para o PC (F9.9, `fora-do-jail --pc`), nunca auto-aprovado; 400 comando vazio/cwd fora do projeto; 401; 429 (3 pendentes por sessão, 20 no total) |
| GET | `/v1/exec/{id}?wait=<s≤55>` | long-poll até estado final → `{…, state, result}`; pedido de outra sessão = 404 |
| POST | `/v1/diagnostico` | `{comando}` → `{comando, saida, codigo, erro?, truncado}`; 400 comando fora da lista; 401 |

Estados: `pending → running → done | failed`, `pending → denied | expired` (10 min sem resposta).
Pedido `target: host` aprovado e sem resultado também expira (timeout do comando + folga):
app fechado no meio da execução não segura o agente para sempre (F9.10).
A fila é só memória (`crates/hubd/src/exec.rs`): reiniciar o hubd perde os pedidos. O
comando roda com `bash -lc` na pasta pedida (dentro do projeto), env do perfil, stdin vazio,
num grupo de processos próprio (timeout padrão 10 min, teto 60, mata o grupo), guardando o fim
da saída (64 KiB por stream). Casou exatamente com `allowed_commands` do projeto? Roda sem
perguntar (`auto_approved`) — exceto pedido `target: host`, que nunca roda sozinho: o hubd não
executa nada no PC, só o app após a sua aprovação (F9.9–F9.10).

## Diagnóstico da VM (F14.1)

`POST /v1/diagnostico` (token da sessão, como o resto da API dos agentes) é **leitura**: não
escreve nada, então não passa pela aprovação do app (F9.7). O corpo é `{"comando": "…"}` e o
comando vem de uma lista fechada, escrita em Rust (`crates/hubd/src/api/diagnostico.rs`): o
agente escolhe a leitura, não o que roda. Fora da lista, a rota responde 400.

| Comando | O que lê |
|---|---|
| `journal-hubd [linhas]` | `journalctl --no-pager -u hubd -n <linhas>` (padrão 200, teto 5000) |
| `tmux-ls` | as sessões do tmux (nome, perfil, projeto, agente) |
| `tmux-pane <sessão>` | `tmux capture-pane -p -t =<sessão>: -S -200` de uma sessão **deste projeto** |
| `explorar` | os prompts de `~/.assistente/perfis/<perfil>/explorar/` e o tamanho de cada um |
| `claude-login` | `bash -l -c 'command -v claude'` (o `claude` está no PATH de login?) |

A saída é cortada em 64 KiB (fica o fim, como no `fora-do jail`) e a resposta traz `codigo`
(o código de saída do comando) e `erro` (o stderr). `tmux-pane` só responde com o painel de uma
sessão do projeto de quem pergunta: sessão viva de outro projeto = 400; sessão que não existe =
404. O cliente do agente é `fora-do-jail diagnostico <comando>`, que devolve a saída no stdout
(sem passar pelo app).

## Protocolo do terminal (F2.3)

- O hubd abre um PTY (`portable-pty`) rodando `tmux attach -t <id>` por conexão WS; vários
  espectadores = vários attach.
- Mensagens **binárias** = bytes do terminal (nos dois sentidos).
- Mensagens **texto** = controle JSON. UI → hubd: `{"type":"input","data":"…"}` (o plugin de
  WebSocket do Tauri manda texto mais fácil que binário) e `{"type":"resize","cols":120,"rows":40}`.
  hubd → UI: `{"type":"exit"}` quando o attach termina, `{"type":"exit","status":N}` quando o
  processo da sessão morreu (F14.3, `N` é o código de saída) e `{"type":"error","message":"…"}`.
- Fechar o WS só desanexa; a sessão continua no tmux.
- Token no WS: o WebSocket do navegador não manda header, então vai em `?token=` só nessa
  rota (tráfego host↔VM, rede privada).
- **Sessão já morta (F14.3):** o tmux rola o painel uma linha quando o processo morre, então a
  tela do painel morto perde a primeira linha da saída. O hubd não abre terminal nesse caso:
  manda o histórico do painel (`capture-pane`) como bytes do terminal e o `exit` com o status.
  Com a aba aberta e o processo morrendo na hora, a saída chega viva e o hubd pergunta ao
  tmux, de meio em meio segundo, se o processo ainda vive.
- **`exit` sem status:** o `exit` com `status` só sai quando há código de saída (processo
  morto por sinal, ou o tmux sem saber o status). Aí a aba avisa só que a sessão acabou.

## Pegadinhas do tmux (3.4)

- `set-option -t` espera um **painel**: `=nome` dá "no such session"; use `=nome:`.
  `kill-session`/`attach-session` aceitam `=nome`.
- Em `list-sessions -F`, TAB e outros caracteres de controle viram `_`: o separador é `|~|`.
- `remain-on-exit` é opção de **janela**: `set-option -w -t =sessão: remain-on-exit on`.
  Para valer desde o nascimento, encadeia no mesmo tmux (`new-session … \; set-option …`):
  quem chega depois corre o risco de o processo já ter morrido.
- Quando o processo morre, o tmux **rola o painel uma linha**: a tela perde a primeira linha
  da saída (o histórico tem tudo).
- **O tmux 3.4 perde o `SIGCHLD` do painel morto em ~5% das vezes:** o processo fica zumbi e
  o `pane_dead_status` não aparece nunca (o status só é mostrado quando o `PANE_STATUSREADY`
  foi marcado, o que o `SIGCHLD` perdido não faz). O `waitpid` do servidor recolhe todos os
  filhos que morrem, então um `run-shell` (que morre na hora) faz ele recolher o zumbi
  esquecido — e o status de saída aparece. O hubd faz isso antes de dizer que o painel
  morreu sem status (`Tmux::reap`).
- `pane_dead_status` é o **código de saída**; um processo morto por sinal não tem código de
  saída (o `pane_dead_signal` é que diz o sinal). Nesse caso a aba avisa só que acabou.

## Testes

- Unitários das rotas com `tower::ServiceExt::oneshot` (já existem).
- Fora do jail (F9.5–F9.6): `agent_asks_user_approves_and_gets_the_output` e
  `client_script_round_trip` sobem as duas APIs com tmux real e rodam o `vm/fora-do-jail` de
  verdade (aprovado, negado, token morto, pedido perdido).
- `crates/hubd/src/sessions.rs` e `api.rs`: tmux real num socket temporário (o devenv tem tmux),
  com `bash` no lugar do agente; o teste de ponta a ponta sobe o hubd numa porta livre, cria
  a sessão por HTTP, conecta no WebSocket, digita `echo marca-$((40+2))` e espera `marca-42`.
- F14.3: `dead_session_shows_the_output_and_the_exit_status` (sessão que sai com 127: a aba
  recebe a saída e o `exit` com o status) e `session_that_dies_with_the_tab_open_sends_the_status`
  (o processo morre com a aba aberta); no app, `TerminalView.test.tsx` com a mensagem de fim.
  Em `sessions.rs`: `command_that_exits_at_once_leaves_the_dead_pane` (o painel fica, com a
  saída e o status), `pane_state_knows_the_status_of_a_dead_pane` (o status sai mesmo quando o
  tmux perde o `SIGCHLD`) e `pane_state_tells_alive_from_dead` (painel vivo, e painel morto
  por sinal: sem código de saída).
