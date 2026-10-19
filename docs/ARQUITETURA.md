# Arquitetura

## Visão geral

```
 PC (Pop!_OS)                                           Notebook (Linux Mint, sempre ligado)
 ┌──────────────────────────────────────────┐            ┌───────────────────────────────┐
 │ App Tauri "Assistente" (host)            │            │ 9router :20128 (já existe)    │
 │  - UI React + xterm.js                   │            │ ai-memory serve :49374        │
 │  - comandos de host: estado da VM,       │            │   (principal, wiki em git)    │
 │    abrir projeto no Cursor               │            │ opencode + ai-jail            │
 │        │ HTTP/WS + token                 │            │ notebook/dorme (loop, tmux)   │
 │        ▼ 10.77.0.10:7878                 │            │ ~/assistente/projetos/<p>/<x> │
 │ ┌─ VM Incus "assistente" ──────────────┐ │   SSH      └──────────────▲────────────────┘
 │ │ hubd (axum): sessões, perfis, notebook├─┼── túnel -L 20128, -L 49374 ┘
 │ │ tmux → ai-jail → claude | opencode   │ │   + git push/fetch + start/stop do dorme
 │ │ ai-memory (cliente: MCP + hooks)     │ │
 │ │ docker (para ./dev dos projetos)     │ │
 │ └──────────────────────────────────────┘ │
 │ Pastas do host montadas na VM (virtiofs), no MESMO caminho e UID:            │
 │   raízes dos perfis, ~/.config/assistente (ro), espelho da memória           │
 └──────────────────────────────────────────────────────────────────────────────┘
```

## Componentes

| Componente | Onde roda | Código | Papel |
|---|---|---|---|
| App desktop | host | `app/` (Tauri 2 + React 19) | UI; comandos de host (`hub_connection`, `vm_status`, `open_in_cursor`) |
| hub-core | host e VM | `crates/hub-core` | Domínio puro: config, perfis, projetos, montagem de sessões, prompt do modo dormir |
| hubd | VM | `crates/hubd` (axum) | API HTTP/WS; dono das sessões tmux, do túnel SSH, do espelho da memória e do modo dormir |
| CLI do host | host | `bin/assistente` (bash) | Criar/ligar/recriar a VM, logins, token, backup, setup do notebook |
| Instalador | host | `instalar.sh` | Bootstrap pós-formatação |
| Provisionamento | VM | `vm/` | cloud-init + `provisionar.sh` + unidade systemd do hubd |
| Notebook | notebook | `notebook/` | `instalar.sh` (sshd, ai-jail, ai-memory) e `dorme` (loop noturno) |
| Ambiente de build | container | `dev`, `devenv/` | Tudo compila no Docker; nada é instalado no host |

## Fluxos

**Abrir uma sessão (F2).** A UI faz `POST /api/sessions {profile, project, agent}`. O hubd
monta um `LaunchPlan` (hub-core) e roda `tmux new-session -d` com `ai-jail --rw-map
~/.assistente/perfis/<perfil> -- claude` e o env do perfil. A UI abre o WebSocket
`/api/sessions/{id}/terminal`, e o hubd faz `tmux attach` num PTY e repassa os bytes. Fechar
a aba não mata a sessão: ela continua no tmux até alguém mandar `DELETE`.

**Notebook (F4).** O hubd mantém `ssh -N -L 20128:127.0.0.1:20128 -L 49374:127.0.0.1:49374
<notebook>` com reconexão. Na VM, os agentes falam com `127.0.0.1`, como hoje no PC.

**Memória (F5).** Hooks e MCP do ai-memory nos agentes da VM apontam para
`127.0.0.1:49374`, que pelo túnel chega ao servidor no notebook. O agente noturno usa o
servidor local do notebook. Periodicamente, o hubd faz `git fetch` do wiki do servidor para o
espelho no host.

**Hora de dormir (F6).** O hubd faz `git push` do projeto para o clone do notebook
(`refs/remotes/pc/*`), cria `dorme/<data>`, exporta um briefing do ai-memory, gera o prompt e
inicia `dorme` num tmux do notebook. De manhã, "Recolher" faz `git fetch` de `dorme/<data>` e
`agente/*` para `refs/remotes/notebook/*` e mostra commits, RELATORIO.md e o merge.

**Auto-atualização (F8).** A aba Assistente é uma sessão de agente no próprio repo. Com
"Aplicar", o hubd lança `vm/atualizar.sh` fora do jail: ele faz um worktree limpo do commit,
roda `just check`, compila a UI e o hubd em `~/.assistente/versoes/<commit>` e troca o link
`atual`. O hubd reinicia sem matar o tmux, e o app recarrega a UI servida pelo hubd. A casca
Tauri ainda precisa de `assistente atualizar` (spec 09).

## Decisões

1. **VM Incus em vez de Docker.** O ai-jail (bubblewrap + Landlock) não foi feito para rodar
   dentro do Docker; numa VM ele funciona igual ao host. Incus cria VMs por CLI com
   cloud-init, o que deixa a VM descartável e fácil de recriar.
2. **Mesmo caminho e mesmo UID na VM.** Um caminho só serve para o hubd, o Cursor do host e
   o git, e evita tradução de caminhos e problemas de dono de arquivo.
3. **Terminais reais (tmux + xterm.js) em vez de chat próprio.** Assim os agentes mantêm
   todos os recursos e atualizações sem retrabalho; o tmux dá persistência de graça.
4. **Túnel SSH em vez de expor portas na LAN.** Já vem criptografado e autenticado, o 9router
   e o ai-memory continuam ouvindo só em loopback no notebook, e funciona pela LAN ou pelo
   Tailscale sem mudar nada além do `ssh_host`.
5. **Servidor do ai-memory no notebook.** O notebook fica sempre ligado; o PC pode desligar à
   noite sem deixar o agente noturno sem memória. O PC guarda um espelho (o wiki é git).
6. **Git direto entre as máquinas, sem GitHub.** O modo dormir nunca faz push para remotos
   públicos, e o WIP não sai da rede. O merge de volta é sempre decisão humana.
7. **Uma VM, perfis separados.** O isolamento entre perfis é por dados (pastas por perfil
   fora dos mounts padrão do ai-jail, env próprio), não por máquina. Se o trabalho exigir mais,
   dá para virar duas VMs: a config já é por perfil.
8. **Convenções do projeto game.** `./dev`, versões exatas com 14 dias de idade (crates e
   npm), `just check`, código em inglês e docs/CLI em pt-BR.

## Riscos e pontos a validar

| Ponto | Quando | Como validar |
|---|---|---|
| Caminho do wiki e do token do ai-memory; spool offline do cliente | F5.1 | Ler `docs/` do ai-memory v2 inteiro e testar com o notebook desligado |
| Flags remotas de `install-mcp`/`install-hooks` para opencode | F5.2 | `ai-memory install-mcp --help` na VM |
| Hooks do ai-memory dentro do ai-jail (binário e token visíveis) | F3.1/F5.2 | Sessão real com `--dry-run --verbose` |
| virtiofs do Incus 6.0 (Ubuntu 24.04) com UID igual, sem `shift` | F1.2 | Criar arquivo na VM e conferir o dono no host |
| `CLAUDE_CONFIG_DIR`/`OPENCODE_CONFIG` isolam login e histórico por perfil | F3.2 | Logar em perfis diferentes e conferir as pastas |
| 9router ouvindo em 127.0.0.1:20128 no notebook | F4.1 | `instalar.sh` do notebook confere |
| docker.sock montado no ai-jail = root dentro da VM | F3.1 | Aceito: a VM é a fronteira; documentado na spec 05 |
