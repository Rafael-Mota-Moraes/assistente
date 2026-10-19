# 05 — Sandbox (ai-jail)

Cobre R5. Funcionalidades F3.x. Ferramenta: <https://github.com/akitaonrails/ai-jail>.

## Camadas

```
host (Pop!_OS)
 └─ VM Incus ............ fronteira principal: kernel próprio, só vê as raízes montadas
     └─ ai-jail ......... bubblewrap + Landlock: só o projeto (rw) e dotdirs de ferramentas
         └─ claude | opencode
```

Por que as duas: a VM protege o host (e o resto do seu home) mesmo que o agente ou uma
dependência escape do ai-jail; o ai-jail protege o resto da VM, inclusive as credenciais dos
outros perfis, e mantém o comportamento igual ao do notebook.

## Como a sessão é lançada (ai-jail 2.10, validado em 2026-10-19)

O ai-jail 2.x é bem mais restrito que o do artigo (v0.4): **home privada** (tmpfs; só os
dados do próprio agente entram), **rede filtrada** (proxy com lista de hosts: api.anthropic.com
no preset do claude, registries de pacotes, github), **ambiente limpo** e **Docker desligado**.
O `LaunchPlan` (`hub_core::session::jail_argv`) monta:

| Flag | Por quê |
|---|---|
| `--rw-map ~/.assistente/perfis/<perfil>` | dados do perfil (opencode, chave SSH); só o perfil ativo |
| `--claude-dir …/<perfil>/claude` (só claude) | `CLAUDE_CONFIG_DIR` do perfil: login, settings, hooks e MCP próprios |
| `--forward-port 20128 --forward-port 49374` | o túnel do notebook em 127.0.0.1; sem isso o proxy responde 405 |
| `--allow-host <host>` (só opencode) | `opencode_allow_hosts` do perfil: API de provedores fora do 9router. Soma à lista do preset (validado: o `registry.npmjs.org` continua liberado) |
| `--map ~/.local/lib/ai-memory` | binário que os hooks chamam |
| `--rw-map ~/.local/share/ai-memory` | spool local dos hooks (notebook fora do ar) |
| `--map <raiz do perfil>/.ai-memory.toml` | marcador do workspace; sem ele a memória cai em `default` |
| `--terminal-passthrough` | o proxy de terminal do ai-jail descarta o OSC 52: sem isso copiar com o mouse no claude/opencode não chega ao app. O agente passa a escrever direto no painel do tmux, que só repassa ao app o OSC 52 de escrita |
| `--env NOME` (uma por variável do perfil) | o valor vem do env do tmux, não aparece na linha de comando |
| `-- claude` / `-- opencode --standalone` | o preset vale com ou sem `--`. O opencode 2 é cliente de um servidor em segundo plano compartilhado, que não sobrevive ao jail: `--standalone` sobe um por sessão |

O preset do opencode monta `~/.config/opencode` e `~/.local/share/opencode`, que são
**compartilhados entre perfis** (config global com o 9router e histórico). Isolar o histórico
por perfil (`XDG_DATA_HOME`) fica para depois.

## Docker dentro da sessão

No 2.x o Docker vem **desligado** (`--docker` liga; equivale a root na VM). Projetos com
`./dev` (como o game) precisam dele para o agente rodar `just check`: ligue por projeto com
`[jail] docker = true` no `.assistente.toml` (F3.1), não globalmente.

## Permissões dos agentes (F3.3)

`ai-jail --bootstrap` gera, por perfil (com `CLAUDE_CONFIG_DIR`/`OPENCODE_CONFIG` do perfil):

- Claude: permite git status/diff/log, cargo, npm, python; nega `rm -rf`, `sudo`,
  `chmod 777`, `git push --force`; pergunta em `git push`, `rm`, `docker run`.
- OpenCode: permissões de bash/edit/write.

No notebook (modo dormir), o `opencode run --auto` roda dentro do ai-jail do mesmo jeito
(`AGENTE_JAIL`, padrão `ai-jail`).

## Pontos a validar

- Hooks do ai-memory dentro do ai-jail: o binário (`~/.local/bin`?) e o token precisam estar
  visíveis; talvez um `ro_map` do diretório de dados do ai-memory.
- `CLAUDE_CONFIG_DIR` fora de `~/.claude` funciona com o instalador nativo e com os hooks.
- Landlock no kernel da imagem cloud do Ubuntu 24.04 (ABI V3+).
