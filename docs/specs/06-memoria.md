# 06 — Memória (ai-memory 2.0)

Cobre R4. Funcionalidades F5.x. Ferramenta: <https://github.com/akitaonrails/ai-memory>
(artigo: akitaonrails.com, "AI Memory 2.0", 2026-09-02).

## O que é

Servidor de memória de longo prazo para agentes: captura o que acontece nas sessões (hooks),
consolida em um wiki Markdown **versionado em git** (fonte da verdade; o SQLite é índice
derivado) e entrega um briefing do projeto a cada sessão nova (MCP). Tem web UI, auth por
token, workspaces e embeddings locais (all-MiniLM-L6-v2, sem GPU). Porta padrão 49374.

## Topologia escolhida

```
notebook:  ai-memory serve  127.0.0.1:49374   ← principal (sempre ligado)
           opencode (modo dormir) → local
VM do PC:  claude/opencode → 127.0.0.1:49374 ──túnel SSH──► notebook
host PC:   memory.mirror_dir  ← git fetch periódico do wiki (cópia + backup)
```

- **Servidor no notebook** porque o notebook fica sempre ligado e o PC pode desligar à noite.
- **Escuta só em loopback** no notebook; o PC chega pelo túnel do hubd (spec 07). Não precisa de
  TLS nem `AI_MEMORY_ALLOWED_HOSTS` para a LAN. Token mesmo assim (`ai-memory init`).
- **Cópia no PC**: o hubd faz `git fetch` do repositório do wiki (por SSH) para
  `memory.mirror_dir` a cada `mirror_every_minutes` (F5.4). Esse diretório fica no host,
  montado na VM, e entra no `bin/assistente backup`.
- **Notebook offline**: o cliente do ai-memory guarda as capturas localmente (spool) e envia
  depois (não usar `--require-server`). Briefing indisponível até voltar. Servir um servidor
  local a partir do espelho fica para "Depois".

## Ligando os agentes

Na VM, por perfil (F5.2), com o env do perfil:

```bash
export AI_MEMORY_SERVER_URL=http://127.0.0.1:49374
export AI_MEMORY_AUTH_TOKEN=<token do servidor>
CLAUDE_CONFIG_DIR=~/.assistente/perfis/<p>/claude ai-memory install-mcp   --client claude-code --apply
CLAUDE_CONFIG_DIR=~/.assistente/perfis/<p>/claude ai-memory install-hooks --agent  claude-code --apply
OPENCODE_CONFIG=…/opencode.json ai-memory install-mcp   --client opencode --apply
OPENCODE_CONFIG=…/opencode.json ai-memory install-hooks --agent  opencode --apply
```

No notebook: `ai-memory setup --yes --agent opencode` (feito pelo `notebook/instalar.sh`).

## Workspaces (pessoal × trabalho)

O hubd escreve um marcador na raiz de cada perfil (F5.3):

```toml
# <raiz do perfil>/.ai-memory.toml
workspace = "pessoal"   # = profiles.<id>.memory_workspace
```

O projeto vem do remote git (`origin`) ou do nome da pasta. A identidade usa o nome do
checkout, não o caminho absoluto, então o mesmo repo no PC (`~/Documentos/…/game`) e no
notebook (`~/assistente/projetos/pessoal/game`) cai no **mesmo projeto**. Isso é essencial
para o modo dormir continuar do contexto do dia.

## Estado real (2026-10-19, ai-memory 3.1.1)

- Notebook: `ai-memory setup --yes --agent open-code` (na v3 o agente é `open-code`, não
  `opencode`) criou o serviço `--user`, plugin + MCP do opencode, e roda com `--enable-web`.
- Escuta em `127.0.0.1:49374`; saúde em `/healthz`; web UI em `/web`.
- Token: `~/.local/share/ai-memory/auth-tokens/default` no notebook (dono-só-leitura). É ele
  que os clientes da VM vão usar (F5.2).
- Da VM, pelo túnel: `curl 127.0.0.1:49374/healthz` → `{"status":"ok"}`.

## Pontos a validar (F5.1)

- Caminho exato do wiki git e do token no diretório de dados (a doc lida na pesquisa estava
  truncada).
- Flags `--server-url`/`--auth-token` em `install-*` para opencode.
- Se `install-hooks` respeita `CLAUDE_CONFIG_DIR` ou se é preciso `--scope project`.
- Spool offline: onde fica e como é reenviado.
