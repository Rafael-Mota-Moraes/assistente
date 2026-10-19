# AGENTS.md

Hub pessoal de agentes de IA: app Tauri (host) + `hubd` (Rust/axum, numa VM Incus) +
notebook (9router, ai-memory, modo dormir). O usuário fala português; responda em pt-BR.

- **O que** e **por quê**: `docs/VISAO.md` (requisitos R1–R9)
- **Como**: `docs/ARQUITETURA.md` e `docs/specs/`
- **O que fazer agora**, com IDs (F2.2, F6.1…): `docs/FUNCIONALIDADES.md`

## Regra nº 1: ambiente isolado

Nada é instalado no computador do usuário. Build, teste e ferramentas rodam no container via
`./dev <comando>` (ex.: `./dev just check`). Só o `git` roda no host. Faltou ferramenta? Ela
entra em `devenv/Dockerfile`. Nunca rode `bin/assistente vm …`, `instalar.sh` ou
`notebook/instalar.sh` sem o usuário pedir: eles instalam coisas e criam a VM de verdade.

## Comandos

| Comando | Uso |
|---|---|
| `./dev just check` | todas as verificações; rode antes de dizer que algo está pronto |
| `./dev just hubd` | hubd em http://127.0.0.1:7878 com `config/assistente.exemplo.toml` |
| `./dev just app-dev` | app Tauri com recarga, falando com o hubd acima |
| `./dev just build` | `.deb`/AppImage em `target/release/bundle/` |
| `./dev just idade-crates` | crates do Cargo.lock com ≥ 14 dias |

## Mapa do código

- `crates/hub-core`: domínio puro, sem I/O de rede. `config.rs` (assistente.toml e validação),
  `project.rs` (descoberta de repos), `session.rs` (`LaunchPlan`: tmux → ai-jail → agente, env
  por perfil), `sleep.rs` + `sleep_prompt.md` (`.assistente.toml` e prompt do modo dormir).
- `crates/hubd`: `main.rs` (CLI, token, servidor), `api.rs` (rotas; pendentes respondem 501
  com o ID da funcionalidade).
- `app/src-tauri`: casca fina com os comandos de host (`hub_connection`, `vm_status`,
  `vm_start`, `vm_stop`, `open_in_cursor`).
- `app/src`: React. `bridge/hub.ts` (cliente do hubd), `bridge/host.ts` (comandos Tauri),
  `bridge/types.ts` (espelho dos tipos da API), `store.ts` (zustand), `features/<área>/`.
- `bin/assistente`, `instalar.sh`, `vm/`: host e VM. `notebook/`: setup e loop `dorme`.

## Regras

- Código, identificadores e chaves de config em inglês; docs, mensagens de UI/CLI e commits
  em pt-BR.
- Dependências com versão exata (`=` no Cargo, sem `^` no package.json) e publicadas há pelo
  menos 14 dias (`just idade-crates`; npm via `minimumReleaseAge` em `app/pnpm-workspace.yaml`).
- Tipos da API mudaram? Atualize `crates/hubd/src/api.rs` e `app/src/bridge/types.ts` juntos
(inclui `/api/memory`, F5.0).
- Rota nova começa como `not_yet("Fx.y")` e aponta para o item do backlog.
- Scripts bash passam no `shellcheck` (está no `just check`).
- Terminou um item do backlog? Marque `[x]` em `docs/FUNCIONALIDADES.md` no mesmo commit.
