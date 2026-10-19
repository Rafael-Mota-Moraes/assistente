# Funcionalidades

Backlog com IDs. Cada item tem **Pronto quando** e **Como testar**. É o formato que o modo
dormir lê: o agente pega o primeiro `- [ ]` e marca `[x]` só com `./dev just check` verde.
Itens marcados *(humano)* dependem de você (VM real, notebook, logins) e o agente não pega.

## F0 — Fundação

- [x] **F0.1 Esqueleto do repo.** Workspace Rust, app Tauri, `./dev`, justfile, docs.
  Pronto quando: `./dev just check` passa. Como testar: `./dev just check`.
- [x] **F0.2 hub-core.** Config, perfis, descoberta de projetos, `LaunchPlan`, prompt do modo dormir.
  Pronto quando: testes cobrem o exemplo de config e a montagem por perfil. Como testar: `./dev cargo test -p hub-core`.
- [x] **F0.3 hubd mínimo.** `/api/health` aberto; `/api/profiles`, `/api/projects`, `/api/notebook` com token; o resto em 501 com o ID.
  Pronto quando: testes de rota passam. Como testar: `./dev just hubd` e `curl 127.0.0.1:7878/api/health`.
- [x] **F0.4 Casca da UI.** Lateral com perfis e projetos, área de sessões vazia, barra de status, botão de dormir desativado.
  Pronto quando: a janela mostra a saúde do hubd. Como testar: `./dev just hubd` e, em outro terminal, `./dev just app-dev`.

## F1 — VM

- [ ] **F1.1 instalar.sh no PC zerado** *(humano)*. Instala Incus/QEMU, cria a config, token, VM e o app.
  Pronto quando: num Pop!_OS limpo, `./instalar.sh` termina e o app abre conectado. Como testar: VM de teste com Pop!_OS.
- [x] **F1.2 Criação da VM** *(humano)*. Validado em 2026-10-19 no Pop!_OS 24.04 (Incus 6.0.0). `bin/assistente vm criar`: rede `assistente0`, IP fixo, cloud-init, montagens virtiofs.
  Pronto quando: `incus exec assistente -- touch <raiz>/x` cria arquivo com seu dono no host. Como testar: o próprio comando.
- [ ] **F1.3 Provisionamento.** `vm/provisionar.sh` com versões fixas e checksums (sem `latest`).
  Pronto quando: rodar duas vezes seguidas não falha nem reinstala nada. Como testar: `incus exec … bash provisionar.sh` duas vezes.
- [ ] **F1.4 hubd como serviço.** systemd, logs no journal, reinício em falha.
  Pronto quando: `systemctl restart hubd` não derruba as sessões tmux. Como testar: abrir sessão, reiniciar, reconectar.
- [ ] **F1.5 Estado da VM no app.** Botões ligar/desligar na barra de status (comando de host).
  Pronto quando: com a VM parada, o app mostra "VM parada" e liga com um clique. Como testar: `incus stop` e usar o app.

## F2 — Sessões

- [ ] **F2.1 Perfis e projetos completos.** Cor e rótulo na UI, projetos por perfil, contagem de sessões por projeto.
  Pronto quando: trocar de perfil troca a lista sem misturar. Como testar: vitest do ProjectList + teste de rota.
- [ ] **F2.2 Ciclo de vida das sessões.** `POST/GET/DELETE /api/sessions` com tmux; lista sobrevive a restart do hubd (lê `tmux ls`).
  Pronto quando: os testes de integração com tmux real (no devenv) passam. Como testar: `./dev cargo test -p hubd`.
- [ ] **F2.3 Terminal por WebSocket.** `/api/sessions/{id}/terminal`: `tmux attach` em PTY (`portable-pty`), resize, vários espectadores. No app, o WS passa pelo `tauri-plugin-websocket` (mixed content, ver spec 03).
  Pronto quando: `vim` funciona com redimensionamento pela UI. Como testar: abrir sessão `bash` e redimensionar a janela.
- [ ] **F2.4 Abas de terminal na UI.** xterm.js com addons fit e web-links, tema igual ao app, atalhos (Ctrl+Shift+T/W, Ctrl+Tab).
  Pronto quando: abrir, alternar e fechar abas sem perder a sessão. Como testar: vitest + uso manual.
- [ ] **F2.5 Botões Claude / OpenCode.** Habilitar na área de sessões; nome da aba = agente + número.
  Pronto quando: um clique abre o agente no projeto, no perfil certo. Como testar: conferir `echo $CLAUDE_CONFIG_DIR` na sessão.
- [ ] **F2.6 Abrir no Cursor.** Já existe o comando de host; falta feedback de erro (toast).
  Pronto quando: o erro aparece na UI quando o Cursor não está instalado. Como testar: renomear o binário e clicar.

## F3 — Sandbox

- [ ] **F3.1 Sessões dentro do ai-jail.** `LaunchPlan` validado com o opencode (spec 05). Falta: Docker por projeto (`--docker` via `.assistente.toml`) para o `./dev`.
  Pronto quando: `./dev just check` de um projeto roda de dentro da sessão. Como testar: sessão real no projeto game.
- [ ] **F3.2 Dados por perfil.** `~/.assistente/perfis/<perfil>/{claude,opencode,ssh}`; só o perfil ativo entra via `--rw-map`.
  Pronto quando: de uma sessão pessoal, `ls ~/.assistente/perfis/trabalho` falha. Como testar: o próprio comando.
- [ ] **F3.3 Permissões padrão.** `ai-jail --bootstrap` por perfil (Claude: allow/deny/ask; opencode: bash/edit/write).
  Pronto quando: `git push --force` é negado e `git push` pergunta. Como testar: pedir ao agente.

## F4 — Notebook e 9router

- [x] **F4.1 Setup do notebook** *(humano)*. Validado em 2026-10-19 (Linux Mint 22.3). `bin/assistente notebook instalar`: sshd, ai-jail, ai-memory, `dorme`, chave da VM autorizada.
  Pronto quando: `ssh notebook true` funciona de dentro da VM sem senha. Como testar: o próprio comando.
- [x] **F4.2 Túnel gerenciado pelo hubd.** Feito em `crates/hubd/src/tunnel.rs`; reconexão testada matando o `ssh` (volta em ~1 s). Falta o teste automático com sshd no devenv. Processo `ssh -N -L …` com backoff, `ServerAliveInterval` e estado exposto em `/api/notebook` (`tunnel_managed: true`).
  Pronto quando: desligar o Wi-Fi do notebook e religar restabelece o túnel em até 30 s. Como testar: teste com sshd local no devenv + manual.
- [ ] **F4.3 Saúde do notebook na UI.** Pontos para SSH, 9router e ai-memory, com última verificação e erro no tooltip. Trocar o connect TCP por checagem HTTP (outro processo na porta dá falso positivo; ver spec 07).
  Pronto quando: cada serviço caído aparece vermelho. Como testar: parar o 9router no notebook.

## F5 — Memória

- [x] **F5.1 Servidor no notebook** *(humano)*. ai-memory 3.1.1 no notebook (spec 06). ai-memory como serviço em 127.0.0.1:49374 com token; descobrir caminho do wiki e do token.
  Pronto quando: `curl` pelo túnel responde, e a spec 06 tem os caminhos reais. Como testar: `curl 127.0.0.1:49374` na VM.
- [x] **F5.2 Clientes na VM.** `bin/assistente agentes configurar` (`vm/configurar-agentes.sh`); opencode e Claude (assinatura, perfil pessoal) validados de ponta a ponta no jail, com o MCP do ai-memory disponível e a captura no workspace certo. `install-mcp`/`install-hooks` para claude-code e opencode em cada perfil, apontando para o túnel.
  Pronto quando: uma sessão nova recebe briefing do projeto. Como testar: abrir duas sessões seguidas e ver o contexto.
- [x] **F5.3 Workspaces por perfil.** `.ai-memory.toml` na raiz de cada perfil (`workspace = "<memory_workspace>"`), criado por `vm/configurar-agentes.sh` e montado no jail. Validado: sessão do perfil pessoal cai no workspace `pessoal`.
  Pronto quando: notas do trabalho não aparecem no briefing pessoal. Como testar: web UI do ai-memory.
- [ ] **F5.4 Espelho no PC.** Job no hubd: `git fetch` do wiki para `memory.mirror_dir` a cada N minutos; estado em `/api/memory`.
  Pronto quando: o espelho tem os commits do servidor e entra no backup. Como testar: `git -C <espelho> log`.
- [ ] **F5.5 Painel de memória.** No começo, a web UI do ai-memory embutida (pelo túnel); depois, uma busca nativa.
  Pronto quando: dá para ver as páginas do projeto atual sem sair do app. Como testar: manual.

## F6 — Modo hora de dormir

- [ ] **F6.1 `notebook/dorme` generalizado.** Prompt e limites vêm do hubd; roda dentro do ai-jail.
  Pronto quando: com um repo de teste e um prompt fake (`AGENTE_OPENCODE` falso), o loop para nos casos de STOP, RELATORIO, árvore suja e sem progresso. Como testar: `notebook/teste-dorme.sh` (a criar) no devenv.
- [ ] **F6.2 Sync git PC → notebook.** `git push <notebook>:<clone> HEAD:refs/remotes/pc/<base>`, clone criado na primeira vez, branch `dorme/<data>`.
  Pronto quando: o notebook tem exatamente o HEAD do PC sem mexer nas branches dele. Como testar: dois repos locais no devenv.
- [ ] **F6.3 Iniciar e acompanhar.** `POST /api/sleep {profile, project}` exige árvore limpa (ou oferece commit WIP), exporta briefing, gera prompt (`hub_core::sleep::render_prompt`) e inicia `dorme` num tmux do notebook. `GET` mostra o log ao vivo.
  Pronto quando: o PC pode ser desligado e o loop continua. Como testar: manual *(humano)*.
- [ ] **F6.4 Parar.** `POST /api/sleep/stop` cria o arquivo STOP; a iteração atual termina.
  Pronto quando: o loop encerra depois da tarefa em curso. Como testar: teste do F6.1.
- [ ] **F6.5 Recolher de manhã.** `POST /api/sleep/collect`: fetch de `dorme/<data>` e `agente/*` para `refs/remotes/notebook/*`; devolve commits, RELATORIO.md e se dá fast-forward.
  Pronto quando: a UI mostra o resumo e faz o merge com um clique (só fast-forward; senão, avisa). Como testar: repos locais no devenv.
- [ ] **F6.6 Tela "Bom dia".** Modal com o que foi feito por item, falhas, log e o botão de merge.
  Pronto quando: dá para revisar e mesclar sem abrir o terminal. Como testar: vitest com dados fake.

## F7 — Reinstalação

- [ ] **F7.1 Backup completo.** `bin/assistente backup` inclui os logins da VM (`~/.assistente/perfis`) além da config e do espelho.
  Pronto quando: o tar tem tudo que `restaurar` precisa. Como testar: listar o tar.
- [ ] **F7.2 Restaurar do zero** *(humano)*. Roteiro do README seguido num PC formatado.
  Pronto quando: em até 30 min o app está funcionando com memória e logins. Como testar: ensaio real (ou VM de teste).
- [ ] **F7.3 `vm recriar` sem perder nada.** Recriar a VM restaura os logins do backup automaticamente.
  Pronto quando: depois de `vm recriar`, as sessões abrem logadas. Como testar: o próprio comando.

## Depois

- Notificação no celular (ntfy) quando o `dorme` termina ou escreve RELATORIO.md.
- Acompanhar o modo dormir pelo celular (UI web servida pelo hubd através do Tailscale).
- Mais agentes (codex, crush) — o `AgentKind` e o ai-jail já são genéricos.
- Modo offline completo da memória (servidor local a partir do espelho quando o notebook cai).
- Duas VMs (pessoal/trabalho) se o trabalho exigir isolamento total.
