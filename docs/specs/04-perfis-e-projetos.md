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
| `opencode_allow_hosts` (opcional) | `["api.meu-provedor.com"]` | `--allow-host` do ai-jail, só nas sessões do opencode |
| `claude_via_9router` | `false` | `true` = `ANTHROPIC_BASE_URL` aponta para o 9router |

Regras validadas: pelo menos um perfil; id só com `[a-z0-9-]` (vira nome do tmux e pasta);
raízes de perfis **não podem se sobrepor** (senão um projeto seria dos dois); hosts liberados
são só nomes (sem `https://`, porta ou caminho).

### Provedores do opencode do PC

`assistente agentes configurar` copia os `provider` do `~/.config/opencode/opencode.json` do
host (com as chaves, pelo stdin) para o `~/.config/opencode/opencode.json` da VM, ao lado do
9router. Para um perfil usar um deles: `opencode_model = "<provedor>/<modelo>"` e o host da
API em `opencode_allow_hosts` (o proxy do jail bloqueia o resto). Trocou a chave ou o plano no
PC? Rode `assistente agentes configurar` de novo.

## O que é separado por perfil

| Coisa | Como |
|---|---|
| Projetos | raiz própria |
| Identidade git | env da sessão |
| Chave SSH | `GIT_SSH_COMMAND` |
| Login, histórico e settings do Claude | `CLAUDE_CONFIG_DIR=~/.assistente/perfis/<id>/claude` |
| Config e login do opencode | `OPENCODE_CONFIG=~/.assistente/perfis/<id>/opencode/opencode.json` |
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

Exemplo: `config/projeto.exemplo.toml`. Hoje só tem `[sleep]` (spec 08); o parse fica em
`hub_core::sleep::ProjectConfig`. Vai versionado no repo do projeto.
