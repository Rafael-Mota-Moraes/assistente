# Arquitetura

## Visão geral

```
 PC (Pop!_OS)                                           Notebook (Linux Mint, dormir / 9router)
 ┌──────────────────────────────────────────┐            ┌───────────────────────────────┐
 │ App Tauri "Assistente" (host)            │            │ 9router :20128                │
 │  - UI React + xterm.js                   │            │ ai-memory (réplica, dormir)   │
 │  - comandos de host: estado da VM,       │            │ opencode + ai-jail + dorme    │
 │    abrir projeto no Cursor               │            │ ~/assistente/projetos/<p>/<x> │
 │        │ HTTP/WS + token                 │            └──────────────▲────────────────┘
 │        ▼ 10.77.0.10:7878                 │   SSH túnel -L 20128 (+ sync git wiki)
 │ ┌─ VM Incus "assistente" ──────────────┐ │
 │ │ hubd (axum): sessões, túnel, sync    │ │
 │ │ ai-memory serve :49374 (F5.0)       │ │
 │ │ tmux → ai-jail → claude | opencode  │ │
 │ │ docker (para ./dev dos projetos)    │ │
 │ └──────────────────────────────────────┘ │
 │ Pastas do host montadas na VM (virtiofs), no MESMO caminho e UID:            │
 │   raízes dos perfis, ~/.config/assistente (ro), wiki memória (mirror_dir rw) │
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
| Notebook | notebook | `notebook/` | `instalar.sh` (sshd, ai-jail, réplica ai-memory) e `dorme` (loop noturno) |
| Ambiente de build | container | `dev`, `devenv/` | Tudo compila no Docker; nada é instalado no host |

## Fluxos

**Abrir uma sessão (F2).** A UI faz `POST /api/sessions {profile, project, agent}`. O hubd
monta um `LaunchPlan` (hub-core) e roda `tmux new-session -d` com `ai-jail --rw-map
~/.assistente/perfis/<perfil> -- claude` e o env do perfil. A UI abre o WebSocket
`/api/sessions/{id}/terminal`, e o hubd faz `tmux attach` num PTY e repassa os bytes. Fechar
a aba não mata a sessão: ela continua no tmux até alguém mandar `DELETE`.

**Notebook (F4).** O hubd mantém `ssh -N -L 20128:127.0.0.1:20128 notebook` (F5.0 remove
`:49374` do túnel). Na VM, o 9router continua em `127.0.0.1:20128`.

**Memória (F5).** Servidor **ai-memory na VM** em `127.0.0.1:49374`; wiki git em
`memory.mirror_dir` no host (montado na VM). Hooks/MCP nas sessões apontam para esse endereço
local. O notebook guarda **réplica** do wiki para o modo dormir; sync bidirecional (F5.4).
*Legado:* até F5.0, `:49374` ainda pode ser encaminhado ao notebook.

**Hora de dormir (F6).** O hubd faz `git push` do projeto para o clone do notebook
(`refs/remotes/pc/*`), cria `dorme/<data>`, exporta um briefing do ai-memory, gera o prompt e
inicia `dorme` num tmux do notebook. De manhã, "Recolher" faz `git fetch` de `dorme/<data>` e
`agente/*` para `refs/remotes/notebook/*` e mostra commits, RELATORIO.md e o merge.

**Fora do jail (F9).** A aba Terminal abre `bash` na VM sem o ai-jail. A engrenagem do projeto
grava em `~/.assistente/projetos.toml` (fora do repo e do jail) o que afrouxar no `LaunchPlan`.
O agente pede um comando com `fora-do-jail`: POST na API dos agentes (`127.0.0.1:7879`, única
porta do hubd no jail, token por sessão), o app mostra o pedido, você aprova e o hubd roda o
comando na VM e devolve a saída ao agente (long-poll).

**Auto-atualização (F8).** A aba Assistente é uma sessão de agente no próprio repo. Com
"Aplicar", o hubd lança `vm/atualizar.sh` fora do jail: ele faz um worktree limpo do commit,
roda `just check`, compila a UI e o hubd em `~/.assistente/versoes/<commit>` e troca o link
`atual`. O hubd reinicia sem matar o tmux, e o app recarrega a UI servida pelo hubd. A casca
Tauri ainda precisa de `assistente atualizar` (spec 09).

**Apontar e pedir (F12).** O app aponta um elemento na página do preview e abre a caixa de
pedido. O hubd monta o prompt curto (URL, onde, seletor, texto, pedido — uma linha só), grava
o detalhe do pick em `.assistente/picks/<data-hora>.md` (dentro do projeto, fora do git) e
entrega: na sessão de um agente (confere que é de agente, do projeto e está viva; cola no
prompt com `set-buffer` + `paste-buffer`, **sem Enter**, que é do usuário) ou no quadro (card
no backlog, com o texto inteiro).

## Decisões

1. **VM Incus em vez de Docker.** O ai-jail (bubblewrap + Landlock) não foi feito para rodar
   dentro do Docker; numa VM ele funciona igual ao host. Incus cria VMs por CLI com
   cloud-init, o que deixa a VM descartável e fácil de recriar.
2. **Mesmo caminho e mesmo UID na VM.** Um caminho só serve para o hubd, o Cursor do host e
   o git, e evita tradução de caminhos e problemas de dono de arquivo.
3. **Terminais reais (tmux + xterm.js) em vez de chat próprio.** Assim os agentes mantêm
   todos os recursos e atualizações sem retrabalho; o tmux dá persistência de graça.
4. **Túnel SSH em vez de expor portas na LAN.** O 9router (e, no legado, ai-memory) ficam em
   loopback no notebook; o túnel traz só o 9router para a VM. Memória principal não depende do
   notebook ligado (decisão 2026-10-19).
5. **Servidor do ai-memory no PC (VM).** Wiki em `memory.mirror_dir` no host; sessões usam
   servidor local. Notebook recebe **réplica** sincronizada para o modo dormir quando o PC
   desliga (F5.4 + F6).
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
| Servidor ai-memory na VM + wiki em `mirror_dir`; sync notebook | F5.0, F5.4 | Notebook desligado: sessões OK; dormir: réplica no notebook |
| Flags remotas de `install-mcp`/`install-hooks` para opencode | F5.2 | `ai-memory install-mcp --help` na VM |
| Hooks do ai-memory dentro do ai-jail (binário e token visíveis) | F3.1/F5.2 | Sessão real com `--dry-run --verbose` |
| virtiofs do Incus 6.0 (Ubuntu 24.04) com UID igual, sem `shift` | F1.2 | Criar arquivo na VM e conferir o dono no host |
| `CLAUDE_CONFIG_DIR`/`OPENCODE_CONFIG` isolam login e histórico por perfil | F3.2 | Logar em perfis diferentes e conferir as pastas |
| 9router ouvindo em 127.0.0.1:20128 no notebook | F4.1 | `instalar.sh` do notebook confere |
| docker.sock montado no ai-jail = root dentro da VM | F3.1 | Aceito: a VM é a fronteira; documentado na spec 05 |
