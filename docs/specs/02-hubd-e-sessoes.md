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
- O servidor tmux nasce como filho do hubd; `KillMode=process` no systemd faz só o hubd
  reiniciar, sem matar as sessões.

## API

Todas as rotas exceto `/api/health` exigem `Authorization: Bearer <token>`. Erros:
`{"error": "...", "feature"?: "F2.2"}`. CORS liberado para `tauri://localhost`,
`http://tauri.localhost` e `http://localhost:1420`.

| Método | Rota | Status | Corpo / resposta |
|---|---|---|---|
| GET | `/api/health` | pronto | `{ok, version, uptime_seconds}` |
| GET | `/api/profiles` | pronto | `[{id, label, color, root}]` |
| GET | `/api/projects?profile=<id>` | pronto | `[{name, path, has_project_config}]` |
| GET | `/api/notebook` | parcial (F4.2) | `{ssh_host, tunnel_managed, services: [{name, port, up}]}` |
| GET | `/api/sessions` | pronto | `[{id, profile, project, agent, created_at, attached}]` |
| POST | `/api/sessions` | pronto | `{profile, project, agent: "claude"|"opencode"}` → sessão |
| DELETE | `/api/sessions/{id}` | pronto | mata a sessão tmux |
| GET (WS) | `/api/sessions/{id}/terminal` | pronto | ver protocolo abaixo |
| GET/POST | `/api/sleep` | F6.3 | spec 08 |
| POST | `/api/sleep/stop`, `/api/sleep/collect` | F6.4/F6.5 | spec 08 |
| GET | `/api/memory` | F5.0 / F5.4 / F5.6 | saúde local (F5.0); sync do espelho (F5.4); detalhes operacionais (F5.6) |

## Lançamento de uma sessão

`hub_core::session::LaunchPlan::build(config, home, spec, seq)` monta tudo sem efeitos:

- **nome tmux**: `<perfil>--<projeto>--<agente>-<n>` (minúsculo, sem `.`/`:`);
- **cwd**: `<raiz do perfil>/<projeto>` (nome sem `/`, sem começar com `.`);
- **comando**: `ai-jail --rw-map ~/.assistente/perfis/<perfil> -- claude|opencode`;
- **env**: `GIT_AUTHOR_*`/`GIT_COMMITTER_*`, `CLAUDE_CONFIG_DIR`, `OPENCODE_CONFIG`,
  `AI_MEMORY_SERVER_URL`, `ANTHROPIC_BASE_URL` (só com `claude_via_9router`),
  `GIT_SSH_COMMAND` (só com `ssh_key`) e `ASSISTENTE_PROFILE`.

`LaunchPlan::tmux_new_session_argv()` vira `tmux new-session -d -s … -c … -e K=V … -- <comando>`.

## Protocolo do terminal (F2.3)

- O hubd abre um PTY (`portable-pty`) rodando `tmux attach -t <id>` por conexão WS; vários
  espectadores = vários attach.
- Mensagens **binárias** = bytes do terminal (nos dois sentidos).
- Mensagens **texto** = controle JSON. UI → hubd: `{"type":"input","data":"…"}` (o plugin de
  WebSocket do Tauri manda texto mais fácil que binário) e `{"type":"resize","cols":120,"rows":40}`.
  hubd → UI: `{"type":"exit"}` quando o attach termina e `{"type":"error","message":"…"}`.
- Fechar o WS só desanexa; a sessão continua no tmux.
- Token no WS: o WebSocket do navegador não manda header, então vai em `?token=` só nessa
  rota (tráfego host↔VM, rede privada).

## Pegadinhas do tmux (3.4)

- `set-option -t` espera um **painel**: `=nome` dá "no such session"; use `=nome:`.
  `kill-session`/`attach-session` aceitam `=nome`.
- Em `list-sessions -F`, TAB e outros caracteres de controle viram `_`: o separador é `|~|`.

## Testes

- Unitários das rotas com `tower::ServiceExt::oneshot` (já existem).
- `crates/hubd/src/sessions.rs` e `api.rs`: tmux real num socket temporário (o devenv tem tmux),
  com `bash` no lugar do agente; o teste de ponta a ponta sobe o hubd numa porta livre, cria
  a sessão por HTTP, conecta no WebSocket, digita `echo marca-$((40+2))` e espera `marca-42`.
