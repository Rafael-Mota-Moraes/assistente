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
- [x] **F0.5 `.ai-jail` fora do git.** O ai-jail mascara o `.ai-jail` (vazio) dentro do sandbox, e o README dele recomenda o gitignore. Este repo: `git rm --cached` + `/.ai-jail` no `.gitignore`. Todos os projetos: `sleep::exclude_jail_config` põe `/.ai-jail` no `info/exclude` ao abrir sessão e antes de dormir, e o `sync_project` faz o mesmo no clone do notebook (senão o `dorme` parava com árvore suja). Spec 05. Testes: `jail_config_does_not_dirty_the_pc_tree` (inclui worktree) e `jail_config_does_not_dirty_the_notebook_clone`.
  Pronto quando: abrir uma sessão no repo não deixa `.ai-jail` modificado. Como testar: `git status --short` depois de uma sessão.

## F1 — VM

- [ ] **F1.1 instalar.sh no PC zerado** *(humano)*. Instala Incus/QEMU, cria a config, token, VM e o app.
  Pronto quando: num Pop!_OS limpo, `./instalar.sh` termina e o app abre conectado. Como testar: VM de teste com Pop!_OS.
- [x] **F1.2 Criação da VM** *(humano)*. Validado em 2026-10-19 no Pop!_OS 24.04 (Incus 6.0.0). `bin/assistente vm criar`: rede `assistente0`, IP fixo, cloud-init, montagens virtiofs.
  Pronto quando: `incus exec assistente -- touch <raiz>/x` cria arquivo com seu dono no host. Como testar: o próprio comando.
- [x] **F1.3 Provisionamento.** `vm/provisionar.sh` com versões fixas e checksums (ai-jail sha256, opencode sha512, ai-memory via `install.sh` pinado, Claude `install.sh <versão>`). Reexecutável: segunda passada não baixa de novo se a versão já está instalada.
  Pronto quando: rodar duas vezes seguidas não falha nem reinstala nada. Como testar: `incus exec … bash provisionar.sh` duas vezes.
- [x] **F1.4 hubd como serviço.** systemd, logs no journal, reinício em falha. Validado: `KillMode=process`, sessão sobrevive a `systemctl restart hubd`.
  Pronto quando: `systemctl restart hubd` não derruba as sessões tmux. Como testar: abrir sessão, reiniciar, reconectar.
- [x] **F1.5 Estado da VM no app.** Botões Ligar/Desligar na barra de status (`vm_start` / `vm_stop` no Tauri). Teste: `StatusBar.test.tsx`.
  Pronto quando: com a VM parada, o app mostra "VM parada" e liga com um clique. Como testar: `incus stop` e usar o app.

## F2 — Sessões

- [x] **F2.1 Perfis e projetos completos.** Cor e rótulo na UI, projetos por perfil, contagem de sessões por projeto. Testes: `ProjectList.test.tsx` (vitest) e `projects_do_not_mix_between_profiles` (rota).
  Pronto quando: trocar de perfil troca a lista sem misturar. Como testar: vitest do ProjectList + teste de rota.
- [x] **F2.2 Ciclo de vida das sessões.** `POST/GET/DELETE /api/sessions` com tmux; lista sobrevive a restart do hubd (lê `tmux ls`). Feito em `crates/hubd/src/sessions.rs` (metadados em opções `@assistente_*` do tmux).
  Pronto quando: os testes de integração com tmux real (no devenv) passam. Como testar: `./dev cargo test -p hubd`.
- [x] **F2.3 Terminal por WebSocket.** `/api/sessions/{id}/terminal`: `tmux attach` em PTY (`portable-pty`), resize, vários espectadores. No app, o WS passa pelo `tauri-plugin-websocket` (mixed content, ver spec 03). Feito em `crates/hubd/src/terminal.rs`; teste de ponta a ponta digita e lê de volta.
  Pronto quando: `vim` funciona com redimensionamento pela UI. Como testar: abrir sessão `bash` e redimensionar a janela.
- [x] **F2.4 Abas de terminal na UI.** xterm.js com addons fit e web-links, tema igual ao app, atalhos (Ctrl+Shift+T/W/P, Ctrl+Tab, Ctrl+1..9) em `features/sessions/shortcuts.ts`; fechar aba esconde sem matar a sessão (seta na barra reabre); web-links abre pelo comando de host `open_url` (xdg-open, só http/https). Testes: `shortcuts.test.ts` + `SessionArea.test.tsx`.
  Pronto quando: abrir, alternar e fechar abas sem perder a sessão. Como testar: vitest + uso manual.
- [x] **F2.5 Botões Claude / OpenCode.** Habilitar na área de sessões; nome da aba = agente + número. Validado na VM real: opencode no ai-jail com 9router e MCP do ai-memory.
  Pronto quando: um clique abre o agente no projeto, no perfil certo. Como testar: conferir `echo $CLAUDE_CONFIG_DIR` na sessão.
- [x] **F2.6 Abrir no Cursor.** Erro do comando de host vira toast no rodapé (`ui/Toast.tsx`, some em 6 s). Testes: `Toast.test.tsx` + caso no `ProjectList.test.tsx`.
  Pronto quando: o erro aparece na UI quando o Cursor não está instalado. Como testar: renomear o binário e clicar.

## F3 — Sandbox

- [x] **F3.1 Sessões dentro do ai-jail.** `LaunchPlan` validado com o opencode (spec 05). Docker por projeto: `[jail] docker = true` no `.assistente.toml` → `--docker` no ai-jail.
  Pronto quando: `./dev just check` de um projeto roda de dentro da sessão. Como testar: sessão real no projeto game.
- [x] **F3.2 Dados por perfil.** `~/.assistente/perfis/<perfil>/{claude,opencode,ssh}`; só o perfil ativo entra via `--rw-map` (`LaunchPlan`, validação de `ssh_key`). Teste: `jail_hides_other_profile_data_dirs` (ai-jail no devenv).
  Pronto quando: de uma sessão pessoal, `ls ~/.assistente/perfis/trabalho` falha. Como testar: o próprio comando.
- [ ] **F3.3 Permissões padrão.** `ai-jail --bootstrap` por perfil (Claude: allow/deny/ask; opencode: bash/edit/write).
  Pronto quando: `git push --force` é negado e `git push` pergunta. Como testar: pedir ao agente.
- [x] **F3.4 Providers do opencode por perfil.** `opencode_providers` no perfil (padrão `["9router"]`): o `agentes configurar` grava os providers (com as chaves) no `opencode.json` do perfil; o config global da VM fica sem chaves de API (só MCP/plugins). O notebook só é exigido se algum perfil listar `9router`. As `policies` do `opencode.json` do PC vão junto (no perfil e no global da VM): o tab de modelos fica igual ao do PC (specs 04, 05).
  Pronto quando: sessão de um perfil só enxerga os próprios providers. Como testar: após `agentes configurar`, `grep apiKey` no config global da VM não acha nada.

## F4 — Notebook e 9router

- [x] **F4.1 Setup do notebook** *(humano)*. Validado em 2026-10-19 (Linux Mint 22.3). `bin/assistente notebook instalar`: sshd, ai-jail, ai-memory, `dorme`, chave da VM autorizada.
  Pronto quando: `ssh notebook true` funciona de dentro da VM sem senha. Como testar: o próprio comando.
- [x] **F4.2 Túnel gerenciado pelo hubd.** Feito em `crates/hubd/src/tunnel.rs`; reconexão testada matando o `ssh` (volta em ~1 s). Falta o teste automático com sshd no devenv. Processo `ssh -N -L …` com backoff, `ServerAliveInterval` e estado exposto em `/api/notebook` (`tunnel_managed: true`).
  Pronto quando: desligar o Wi-Fi do notebook e religar restabelece o túnel em até 30 s. Como testar: teste com sshd local no devenv + manual.
- [x] **F4.3 Saúde do notebook na UI.** Pontos para túnel, 9router e (enquanto legado) ai-memory via túnel. Checagem HTTP em `/api/notebook`. **F5.0:** ai-memory sai daqui; probe local em `/api/memory`.
  Pronto quando: cada serviço caído aparece vermelho. Como testar: parar o 9router no notebook.

## F5 — Memória

- [x] **F5.0 Servidor principal na VM (PC).** `ai-memory serve` via `vm/ai-memory.service`, wiki em `memory.mirror_dir`; `provisionar.sh` + `configurar-agentes.sh` locais; túnel sem `:49374`; `/api/memory` + ponto na UI. Testes: `memory_http_probe_*` + `StatusBar.test.tsx`.
  Pronto quando: `curl 127.0.0.1:49374/healthz` na VM; briefing com notebook desligado. Como testar: `./atualizar.sh`, `agentes configurar`, sessão Claude *(humano)*.
- [x] **F5.1 Servidor no notebook** *(humano, legado)*. Validado 2026-10-19 antes da topologia PC-first. Mantido como réplica/dormir até F5.4.
  Pronto quando: (legado) `curl` pelo túnel responde. Como testar: spec 06 “Validação histórica”.
- [x] **F5.2 Clientes na VM.** `vm/configurar-agentes.sh`; hooks/MCP por perfil. Validado com túnel (legado). **F5.0** passa token/URL do servidor local.
  Pronto quando: briefing em sessão nova. Como testar: duas sessões seguidas.
- [x] **F5.3 Workspaces por perfil.** `.ai-memory.toml` na raiz de cada perfil. Validado.
  Pronto quando: workspaces não se misturam. Como testar: web UI do ai-memory.
- [ ] **F5.4 Sync wiki PC ↔ notebook.** Job no hubd: espelho em `memory.mirror_dir` é a **fonte**; push/fetch com o notebook antes/depois do dormir (F6); estado em `/api/memory`.
  Pronto quando: dormir usa réplica atualizada; backup inclui wiki completo. Como testar: `git -C <espelho> log` + ensaio dormir.
- [ ] **F5.5 Painel de memória.** Web UI do ai-memory embutida apontando para o servidor **local** (VM); depois busca nativa.
  Pronto quando: ver páginas do projeto sem sair do app. Como testar: manual.
- [ ] **F5.6 Acompanhar memória (operacional).** Além do ponto na barra: painel ou drawer na UI (ex.: clique em **ai-memory**) e `GET /api/memory` enriquecido — `data_dir`, `mirror_dir`, saúde, `last_error`, versão do servidor, último commit do wiki (`git log -1` em `mirror_dir/wiki`), estado do sync (F5.4 quando existir), atalho para web UI local (proxy ou embed mínimo). Opcional: `bin/assistente memoria status` no host via hubd.
  Pronto quando: dá para ver por que está vermelho, onde ficam wiki/SQLite e o que mudou no wiki sem SSH na VM. Como testar: parar `ai-memory`, abrir o painel e conferir erro + paths; subir de novo e ver verde com commit recente.

## F6 — Modo hora de dormir

- [x] **F6.1 `notebook/dorme` generalizado.** Prompt e limites vêm por env (o hubd preenche, F6.3); roda dentro do ai-jail (`AGENTE_JAIL`, `""` desliga). `AGENTE_ESPERA` torna a espera entre tentativas configurável. Teste: `notebook/teste-dorme.sh` no `just check` (11 verificações: STOP, RELATORIO antes/depois, árvore suja antes/depois, fora da base, desistência sem progresso, progresso que commita na base).
  Pronto quando: com um repo de teste e um prompt fake (`AGENTE_OPENCODE` falso), o loop para nos casos de STOP, RELATORIO, árvore suja e sem progresso. Como testar: `notebook/teste-dorme.sh` no devenv.
- [x] **F6.2 Sync git PC → notebook.** `crates/hubd/src/sleep.rs` (`sync_project`): `git push --force <notebook>:<clone> HEAD:refs/remotes/pc/<base>`, clone criado na primeira vez (`git init`), `switch -C dorme/<data>`. `Remote::Ssh` (produção, `bash -s` no stdin) / `Remote::Local` (testes); detached HEAD e metacaracteres no caminho/data são recusados. Testes: 3 casos com dois repos locais no devenv (branches do notebook intactas, reset no PC chega via force).
  Pronto quando: o notebook tem exatamente o HEAD do PC sem mexer nas branches dele. Como testar: dois repos locais no devenv.
- [ ] **F6.3 Iniciar e acompanhar.** Feito: `POST /api/sleep` (árvore limpa ou WIP autorizado, sync F6.2, briefing best-effort do ai-memory local via `/api/v1/.../briefing`, prompt do `render_prompt`, `dorme` em tmux no notebook com `-e` env), `GET /api/sleep` (`sleeping` + tail do log), botão "Hora de dormir" ativo (vira "Dormindo… (tentativa N/M)" com parar). Testes locais: ciclo completo no devenv (`sleep_full_cycle_start_status_collect_merge`, agente falso + tmux local) + guards (409 suja/duplicado, WIP) + `SleepButton.test.tsx`. Falta: validação *(humano)* no notebook real — desligar o PC com o loop rodando.
  Pronto quando: o PC pode ser desligado e o loop continua. Como testar: manual *(humano)*.
- [x] **F6.4 Parar.** `POST /api/sleep/stop` cria `<workdir>/STOP` no notebook (`request_stop` em `crates/hubd/src/sleep.rs`, ssh via `bash -s`); 204, ou 502 com o motivo sem notebook. O loop encerra depois da tarefa em curso (caso STOP do `teste-dorme.sh`); `dorme` apaga o STOP ao iniciar. Testes: `request_stop_creates_the_file_and_the_parent_dir` + `sleep_stop_fails_loudly_without_a_notebook`.
  Pronto quando: o loop encerra depois da tarefa em curso. Como testar: teste do F6.1.
- [x] **F6.5 Recolher de manhã.** `POST /api/sleep/collect {profile, project, date}`: fetch de `dorme/<data>` e `agente/*` para `refs/remotes/notebook/*`, devolve commits first-parent, RELATORIO.md (lido na árvore do notebook, não é commitado) e `fast_forward` (`collect_project` em `crates/hubd/src/sleep.rs`). `POST /api/sleep/merge`: só fast-forward; 409 sem mudar nada se a base andou. A UI do resumo é a F6.6; cliente em `bridge/hub.ts`. Testes: round-trip com repos locais no devenv (`collect_fetches_relatorio_and_merges_fast_forward`, `sleep_collect_and_merge_round_trip`).
  Pronto quando: a UI mostra o resumo e faz o merge com um clique (só fast-forward; senão, avisa). Como testar: repos locais no devenv.
- [x] **F6.6 Tela "Bom dia".** Modal `GoodMorning.tsx` (botão "Bom dia" na lateral): noite editável (padrão ontem), commits por item, RELATORIO.md, merge fast-forward com um clique (409 vira aviso) e re-colher. Estado em `store.morning`, ações em `features/sleep/morning.ts`. O log ao vivo entra com a F6.3 (GET /api/sleep). Testes: `GoodMorning.test.tsx` (7 casos com dados fake).
  Pronto quando: dá para revisar e mesclar sem abrir o terminal. Como testar: vitest com dados fake.

## F7 — Reinstalação

- [x] **F7.1 Backup completo.** `bin/assistente backup` puxa `~/.assistente/perfis` da VM (`tar` pelo `incus exec`, dono e modo preservados) para `vm-perfis/` no tar, junto da config e do espelho; `--sem-logins` faz o parcial sem a VM. `restaurar` devolve config e espelho ao home e deixa os logins em `~/.local/share/assistente/restauracao/perfis`, de onde entram na VM no próximo `vm criar` ou via `vm logins` (consumidos depois de copiados). Teste: `bin/teste-backup.sh` (incus falso no `just check`).
  Pronto quando: o tar tem tudo que `restaurar` precisa. Como testar: listar o tar.
- [ ] **F7.2 Restaurar do zero** *(humano)*. Roteiro do README seguido num PC formatado.
  Pronto quando: em até 30 min o app está funcionando com memória e logins. Como testar: ensaio real (ou VM de teste).
- [ ] **F7.3 `vm recriar` sem perder nada.** Recriar a VM restaura os logins do backup automaticamente.
  Pronto quando: depois de `vm recriar`, as sessões abrem logadas. Como testar: o próprio comando.

## F8 — Auto-atualização

Spec 09. Ordem sugerida: F8.1 e F8.2 valem sozinhas; F8.4 começa por uma prova mínima (é o maior risco).

- [x] **F8.1 Terminais reconectam sozinhos.** `connectPersistent` em `bridge/terminal.ts`: WS caiu sem `exit` → nova tentativa com backoff (1 s → 30 s, zera a cada conexão aceita) e faixa "reconectando" no terminal; último resize é reaplicado. Testes: `bridge/terminal.test.ts` (backoff, escalada, `exit` sem reconectar, `close()` no meio do backoff).
  Pronto quando: `systemctl restart hubd` com abas abertas faz os terminais voltarem sozinhos. Como testar: vitest do backoff + manual.
- [x] **F8.2 Estado da UI sobrevive a recarregar.** Perfil, projeto, aba ativa, abas fechadas e último agente no `sessionStorage` (zustand `persist`, chave `assistente-ui`); `usePolling` descarta perfil restaurado que saiu da config. Testes: `store.test.ts`.
  Pronto quando: recarregar a página volta para o mesmo lugar. Como testar: vitest do store.
- [x] **F8.3 hubd serve a UI.** `just build-ui` (app/dist + `commit.txt`); `GET /ui/*` de `~/.assistente/versoes/atual/ui` (`--ui-dir` sobrescreve), aberto sem token; `commit` em `/api/health` (hubd via `build.rs`, UI via `commit.txt`). Testes: `serves_the_built_ui_without_token` + `without_ui_dir_the_route_is_gone_and_commit_is_null`.
  Pronto quando: o teste de integração baixa `/ui/` e o `index.html` aponta para os assets servidos. Como testar: `./dev cargo test -p hubd`.
- [ ] **F8.4 Casca carrega a UI do hubd** *(humano)*. Capability `remote.urls`, comando `host_api_version`, UI embutida como reserva (hubd fora, UI ausente ou sem "pronta" em 10 s).
  Pronto quando: o app abre com a UI do hubd; com o hubd parado, abre a reserva com o erro. Como testar: app real com a VM.
- [ ] **F8.5 Atualizador na VM.** `vm/atualizar.sh` (worktree do commit, `just check`, build, `versoes/atual`, restart com volta automática); `GET /api/update`, `POST /api/update/apply`, `POST /api/update/rollback`; `hubd.service` com `ExecStart=%h/.assistente/versoes/atual/hubd` e `Restart=always`.
  Pronto quando: aplicar um commit que quebra o `/api/health` volta sozinho para a versão anterior. Como testar: script com repo de teste no devenv + manual na VM.
- [ ] **F8.6 Aba e painel Assistente.** `[self_update]` na config; botão fixo; painel com commit em uso, pendentes, camadas tocadas, Aplicar, log e Voltar.
  Pronto quando: dá para aplicar e voltar sem sair do app. Como testar: vitest com dados fake + manual.
- [ ] **F8.7 Recarregar ao aplicar.** O app nota o commit novo da UI no `/api/health` e recarrega; commit que toca `app/src-tauri` mostra "reinstale o app".
  Pronto quando: aplicar uma mudança de texto na UI aparece sem fechar o app e com os terminais vivos. Como testar: manual na VM.

## Depois

- Notificação no celular (ntfy) quando o `dorme` termina ou escreve RELATORIO.md.
- Acompanhar o modo dormir pelo celular (UI web servida pelo hubd através do Tailscale).
- Mais agentes (codex, crush) — o `AgentKind` e o ai-jail já são genéricos.
- Memória no notebook só como réplica de longa duração se o PC ficar meses off (fora do fluxo normal F5.4).
- Duas VMs (pessoal/trabalho) se o trabalho exigir isolamento total.
