# 04 — Perfis e projetos

Cobre R8 (pessoal × trabalho). Funcionalidades F2.1, F3.2, F5.3.

## Config

Arquivo único `~/.config/assistente/assistente.toml`, lido pelo app (host) e pelo hubd (VM,
montado só leitura). Exemplo comentado: `config/assistente.exemplo.toml`. Parse e validação
em `hub_core::config` (`deny_unknown_fields`: chave errada = erro claro, não silêncio).

Blocos: `[hub]`, `[vm]`, `[notebook]` + `[notebook.tunnel]`, `[memory]`, `[profiles.<id>]`.

### Perfil

| Chave | Exemplo | Uso |
|---|---|---|
| `label`, `color` | `"Trabalho"`, `"#0ea5e9"` | UI |
| `root` | `~/Documentos/trabalho` | onde ficam os projetos; montado na VM no mesmo caminho |
| `git_name`, `git_email` | | `GIT_AUTHOR_*`/`GIT_COMMITTER_*` nas sessões |
| `ssh_key` (opcional) | `~/.assistente/perfis/trabalho/ssh/id_ed25519` | `GIT_SSH_COMMAND` |
| `memory_workspace` | `"trabalho"` | `.ai-memory.toml` na raiz (spec 06) |
| `opencode_model` (opcional) | `"9router/free"` | modelo padrão do opencode no perfil |
| `opencode_small_model` (opcional) | `"meu-provedor/modelo-pequeno"` | `small_model` do opencode (títulos, resumos) |
| `opencode_providers` (opcional) | `["9router", "meu-provedor"]` | providers que o perfil enxerga (F3.4); padrão `["9router"]` |
| `opencode_allow_hosts` (opcional) | `["api.meu-provedor.com"]` | `--allow-host` do ai-jail, só nas sessões do opencode |
| `claude_via_9router` | `false` | `true` = `ANTHROPIC_BASE_URL` aponta para o 9router |

Regras validadas: pelo menos um perfil; id só com `[a-z0-9-]` (vira nome do tmux e pasta);
raízes de perfis **não podem se sobrepor** (senão um projeto seria dos dois); hosts liberados
são só nomes (sem `https://`, porta ou caminho).

### Provedores do opencode do PC

`assistente agentes configurar` copia os `provider` **e as `policies`** do
`~/.config/opencode/opencode.json` do host (com as chaves, pelo stdin) para o `opencode.json`
**de cada perfil** que os lista em `opencode_providers` (F3.4) — o config global da VM fica
sem chaves de API (só MCP, plugins e node_modules), mas recebe as policies também: o tab de
modelos fica igual ao do PC dentro e fora do perfil. Bloqueou um provider no PC? O perfil só
enxerga o que o PC permite; quando o 9router voltar, inclua-o nas policies do PC antes de
listá-lo de novo. Para um perfil usar um provider: liste-o em `opencode_providers`, aponte
`opencode_model = "<provedor>/<modelo>"` e libere o host da API em `opencode_allow_hosts` (o
proxy do jail bloqueia o resto). O notebook só é necessário quando algum perfil lista
`"9router"`. Trocou a chave ou o plano no PC? Rode `assistente agentes configurar` de novo.

## O que é separado por perfil

| Coisa | Como |
|---|---|
| Projetos | raiz própria |
| Identidade git | env da sessão |
| Chave SSH | `GIT_SSH_COMMAND` |
| Login, histórico e settings do Claude | `CLAUDE_CONFIG_DIR=~/.assistente/perfis/<id>/claude` |
| Config e login do opencode | `OPENCODE_CONFIG=~/.assistente/perfis/<id>/opencode/opencode.json` |
| Providers e chaves do opencode | `opencode_providers` → gravados no `opencode.json` do perfil (F3.4); policies do PC espelhadas (tab igual ao do PC) |
| Memória | workspace do ai-memory |
| Combo do 9router | `opencode_model` |
| Visibilidade entre perfis | só a pasta do perfil ativo entra no ai-jail (spec 05) |

**Compartilhado de propósito**: a VM, o 9router, o servidor de memória (com workspaces
separados) e as ferramentas instaladas.

## Projetos

`hub_core::discover_projects(root)`: filhos diretos da raiz que têm `.git` (pasta ou arquivo de
worktree), sem ocultos, em ordem alfabética. Raiz inexistente = lista vazia. Projetos em
subpastas mais fundas ficam para depois (config `scan_depth`, se precisar).

### `.assistente.toml` (por projeto, opcional)

Exemplo: `config/projeto.exemplo.toml`. Parse em `hub_core::sleep::ProjectConfig`:

| Seção | Uso |
|---|---|
| `[jail]` | `docker = true` liga `--docker` do ai-jail na sessão (spec 05, F3.1) |
| `[sleep]` | Modo dormir (spec 08) |

Vai versionado no repo do projeto.
