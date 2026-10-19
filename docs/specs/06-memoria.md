# 06 — Memória (ai-memory 2.0)

Cobre R4. Funcionalidades F5.x. Ferramenta: <https://github.com/akitaonrails/ai-memory>
(artigo: akitaonrails.com, "AI Memory 2.0", 2026-09-02).

## O que é

Servidor de memória de longo prazo para agentes: captura o que acontece nas sessões (hooks),
consolida em um wiki Markdown **versionado em git** (fonte da verdade; o SQLite é índice
derivado) e entrega um briefing do projeto a cada sessão nova (MCP). Tem web UI, auth por
token, workspaces e embeddings locais (all-MiniLM-L6-v2, sem GPU). Porta padrão 49374.

## Topologia (decisão 2026-10-19)

**Fonte da verdade no PC principal** (host + VM), não no notebook.

```
host PC:   memory.mirror_dir  ← wiki git principal (montado rw na VM)
VM:        ai-memory serve  127.0.0.1:49374   ← servidor principal (F5.0)
           claude/opencode → 127.0.0.1:49374  (local, sem túnel)
notebook:  réplica do wiki + ai-memory opcional só para o modo dormir (F5.4 / F6)
           9router :20128 (túnel → VM, spec 07)
```

- **Servidor na VM** porque as sessões de agente rodam lá; o wiki mora em `memory.mirror_dir`
  no host (virtiofs, mesmo caminho), entra no backup e sobrevive a recriar a VM.
- **Notebook desligado** não derruba memória no dia a dia: briefing, MCP e capturas falam com
  o servidor local enquanto PC/VM estiverem ligados.
- **Notebook** continua com 9router e o loop `dorme` (spec 08). Antes/depois do dormir, o wiki
  **sincroniza** entre PC e notebook (F5.4); à noite o agente pode usar cópia local no notebook
  enquanto o PC estiver off.
- **Estado do código (transição):** até **F5.0**, o hubd ainda encaminha `:49374` pelo túnel SSH
  para o ai-memory do notebook (setup antigo). A UI trata ai-memory como serviço do notebook
  (F4.3). Depois de F5.0, saúde e túnel ficam só com o 9router; memória passa para `/api/memory`.

## Ligando os agentes

Na VM, por perfil (F5.2), com o env do perfil — **alvo:** URL local, não túnel:

```bash
export AI_MEMORY_SERVER_URL=http://127.0.0.1:49374
export AI_MEMORY_AUTH_TOKEN=<token do servidor na VM>
CLAUDE_CONFIG_DIR=~/.assistente/perfis/<p>/claude ai-memory install-mcp   --client claude-code --apply
CLAUDE_CONFIG_DIR=~/.assistente/perfis/<p>/claude ai-memory install-hooks --agent  claude-code --apply
OPENCODE_CONFIG=…/opencode.json ai-memory install-mcp   --client opencode --apply
OPENCODE_CONFIG=…/opencode.json ai-memory install-hooks --agent  opencode --apply
```

`bin/assistente agentes configurar` hoje ainda copia token do notebook (legado); F5.0 passa a
gerar/usar token do servidor local.

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

## Notebook offline (PC ligado)

Com F5.0: **sem impacto** nas sessões da VM — servidor local responde.

## PC / VM offline (notebook ligado)

Briefing e capturas no PC param. O modo dormir (F6) usa a **réplica** no notebook após sync;
commits/notas feitos à noite voltam no **Recolher** (F6.5) junto com o código.

## Spool

Hooks montam `~/.local/share/ai-memory` no jail (spec 05) para fila local se o servidor cair
brevemente; com servidor local estável na VM, o caso principal é sync notebook ↔ espelho.

## Validação histórica (2026-10-19, ai-memory 3.1.1 no notebook)

Antes da mudança de topologia, validou-se servidor no notebook + túnel + clientes na VM
(F5.1–F5.3). Caminhos úteis no notebook (réplica / legado):

- Token: `~/.local/share/ai-memory/auth-tokens/default`
- Saúde: `/healthz`; web UI: `/web`
- `ai-memory setup --yes --agent open-code` no notebook (v3)

## Clientes na VM (F5.2, validado em 2026-10-19, URL legada via túnel)

`bin/assistente agentes configurar` → `vm/configurar-agentes.sh`: opencode/9router, token,
hooks/MCP por perfil, `.ai-memory.toml` na raiz. Prova end-to-end com túnel ao notebook.

## Pontos em aberto

- F5.0: systemd do ai-memory na VM, wiki em `mirror_dir`, token local, retirar `:49374` do túnel.
- F5.4: sync git do wiki PC ↔ notebook (não “fetch do notebook como principal”).
- F5.5: painel / web UI apontando para o servidor **local**.
- Spool offline: detalhes na doc upstream do ai-memory 3.x.
