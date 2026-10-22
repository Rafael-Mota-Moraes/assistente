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
- [x] **F2.7 Aviso de login do Claude por perfil.** O login é por perfil (`CLAUDE_CONFIG_DIR`): sem ele a sessão do Claude abria e morria na hora, sem explicação. `GET /api/profiles` traz `claude_login` (`hub_core::claude_logged_in`: `.credentials.json` do perfil ou `claude_via_9router`); a UI mostra a faixa com `bin/assistente login claude <perfil>` e não cria a sessão. Testes: `claude_login_is_per_profile`, `other_routes_need_the_token`, caso no `SessionArea.test.tsx`.
  Pronto quando: clicar em Claude num perfil sem login mostra o comando em vez de uma aba que some. Como testar: vitest + perfil sem `.credentials.json`.

## F3 — Sandbox

- [x] **F3.1 Sessões dentro do ai-jail.** `LaunchPlan` validado com o opencode (spec 05). Docker por projeto: `[jail] docker = true` no `.assistente.toml` → `--docker` no ai-jail.
  Pronto quando: `./dev just check` de um projeto roda de dentro da sessão. Como testar: sessão real no projeto game.
- [x] **F3.2 Dados por perfil.** `~/.assistente/perfis/<perfil>/{claude,opencode,ssh}`; só o perfil ativo entra via `--rw-map` (`LaunchPlan`, validação de `ssh_key`). Teste: `jail_hides_other_profile_data_dirs` (ai-jail no devenv).
  Pronto quando: de uma sessão pessoal, `ls ~/.assistente/perfis/trabalho` falha. Como testar: o próprio comando.
- [x] **F3.3 Permissões padrão.** `ai-jail --bootstrap` por perfil (Claude: allow/deny/ask; opencode: bash/edit/write).
  Pronto quando: `git push --force` é negado e `git push` pergunta. Como testar: pedir ao agente.
- [x] **F3.4 Providers do opencode por perfil.** `opencode_providers` no perfil (padrão `["9router"]`): o `agentes configurar` grava os providers (com as chaves) no `opencode.json` do perfil; o config global da VM fica sem chaves de API (só MCP/plugins). O notebook só é exigido se algum perfil listar `9router`. As `policies` do `opencode.json` do PC vão junto (no perfil e no global da VM): o tab de modelos fica igual ao do PC (specs 04, 05).
  Pronto quando: sessão de um perfil só enxerga os próprios providers. Como testar: após `agentes configurar`, `grep apiKey` no config global da VM não acha nada.
- [x] **F3.5 Login do Claude renova dentro do jail.** O preset do ai-jail só libera `api.anthropic.com`; o endpoint OAuth (`platform.claude.com`, antes `console.anthropic.com`) ficava bloqueado. O token não renovava (401 "OAuth access token has expired" depois de algumas horas) e o `/login` na aba falhava com "proxy refused the connection". O `LaunchPlan` passa `--allow-host` desses hosts nas sessões do Claude com login da Anthropic (não com `claude_via_9router` nem com rede completa). Testes: `claude_gets_the_oauth_hosts`, `full_network_drops_forward_ports_and_host_filter`.
  Pronto quando: o `LaunchPlan` do Claude libera os hosts OAuth. Como testar: `./dev just check`.
- [ ] **F3.6 Validar o login dentro do jail** *(humano)*. Na VM real, o `/login` dentro da aba Claude conclui e uma sessão aberta além da validade do token não mostra o 401.

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
- [x] **F5.4 Sync wiki PC ↔ notebook.** Job no hubd: espelho em `memory.mirror_dir` é a **fonte**; push/fetch com o notebook antes/depois do dormir (F6); estado em `/api/memory`.
  Pronto quando: dormir usa réplica atualizada; backup inclui wiki completo. Como testar: `git -C <espelho> log` + ensaio dormir.
- [x] **F5.5 Painel de memória.** Web UI do ai-memory embutida apontando para o servidor **local** (VM); depois busca nativa. Feito: proxy reverso no hubd (`/web`, `/api/v1` → ai-memory local) que injeta o token do ai-memory (`<memory.data_dir>/auth-tokens/default`), sem pedir login. O botão "Memória" na lateral abre um **painel embutido na área principal** (`open_memory_panel`): webview filha do Tauri ancorada sobre a área de conteúdo, autenticada em `/memoria/entrar?token=` (cookie de sessão); no navegador cai num iframe. Fica fora de `capabilities/default.json`, então a página remota não alcança os comandos do app.
  Pronto quando: ver páginas do projeto sem sair do app. Como testar: manual.
- [x] **F5.6 Acompanhar memória (operacional).** Além do ponto na barra: painel ou drawer na UI (ex.: clique em **ai-memory**) e `GET /api/memory` enriquecido — `data_dir`, `mirror_dir`, saúde, `last_error`, versão do servidor, último commit do wiki (`git log -1` em `mirror_dir/wiki`), estado do sync (F5.4 quando existir), atalho para web UI local (proxy ou embed mínimo). Opcional: `bin/assistente memoria status` no host via hubd.
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
- [ ] **F7.3 `vm recriar` sem perder nada.** Recriar a VM restaura os logins do backup automaticamente. Código pronto (`vm recriar` puxa os logins e `projetos.toml` da VM atual antes de apagá-la); falta a validação na VM real.
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
- [x] **F8.5 Atualizador na VM.** `vm/atualizar.sh` (worktree do commit, `just check`, build, `versoes/atual`, restart com volta automática); `GET /api/update`, `POST /api/update/apply`, `POST /api/update/rollback`; `hubd.service` com `ExecStart=%h/.assistente/versoes/atual/hubd` e `Restart=always`. Testes: `vm/teste-atualizar.sh` (`just teste-atualizar`) com repositório e serviço falsos cobre aplicação, `anterior`, saúde quebrada voltando sozinha, verificação que falha sem mudar nada e a poda preservando os alvos dos links; em `api.rs`, commit inválido (400), atualizador ausente (409), spawn com o commit e a troca `atual`↔`anterior` do rollback (409 sem anterior). Falta só conferir na VM real (manual).
  Pronto quando: aplicar um commit que quebra o `/api/health` volta sozinho para a versão anterior. Como testar: `./dev just teste-atualizar` (o serviço falso simula o reinício e a saúde).
- [ ] **F8.6 Aba e painel Assistente.** `[self_update]` na config; botão fixo; painel com commit em uso, pendentes, camadas tocadas, Aplicar, log e Voltar. A fila de pendentes agora exclui os ancestrais do commit em uso; falta o teste manual de Aplicar e Voltar na VM real.
  Pronto quando: dá para aplicar e voltar sem sair do app. Como testar: vitest com dados fake + manual.
- [x] **F8.7 Recarregar ao aplicar.** O `usePolling` compara o `commit.ui` de `/api/health` (curto ou completo) com o que já estava em uso; quando muda, espera o `running` de `/api/update` cair (o atualizador troca os links antes de reiniciar o hubd), grava o aviso no `sessionStorage`, mostra o toast "versão nova aplicada: recarregando…" e recarrega a página (os terminais reconectam ao tmux). Depois do reload, um banner confirma a versão nova; quando `GET /api/update` responde `shell_changed` (diff `anterior..atual` tocando `app/src-tauri`), o banner pede `assistente atualizar` para reinstalar a casca e só some no X. Testes: `uiUpdate.test.ts` (hashes curtos/longos, aviso idempotente no StrictMode), `usePolling.test.tsx` (recarrega na troca, espera o atualizador e não recarrega quando ele volta), `UiUpdatedBanner.test.tsx` e, no hubd, `touches_shell_only_sees_the_tauri_layer` e `update_status_flags_shell_changes`. Falta conferir na VM real (manual).
  Pronto quando: aplicar uma mudança de texto na UI aparece sem fechar o app e com os terminais vivos. Como testar: vitest com os fakes + manual na VM.
- [x] **F8.8 Juntar commits na main.** Painel Assistente: caixas para marcar commits pendentes e "Aplicar N na main" → `POST /api/update/main`. Faz cherry-pick em ordem de histórico numa worktree descartável e só anda com a `main` no fim (fast-forward se ela é o checkout do projeto; senão troca a ref conferindo o valor antigo). Conflito ou `main` movida: nada muda. Não instala nada (isso continua sendo `apply`). Testes: `apply_to_main_*` no hubd (git real) e `AssistantPanel.test.tsx`.
  Pronto quando: marcar dois commits e aplicar deixa a `main` com os dois, na ordem do histórico. Como testar: `./dev cargo test -p hubd apply_to_main` e vitest do painel.

## F9 — Fora do jail

Spec 05 ("Fora do jail"). O agente roda no ai-jail (Docker desligado, rede filtrada, home
privada): o que ele não consegue rodar ia para um terminal do PC aberto na mão. Ordem: F9.1
sozinha resolve a maior parte; F9.2–F9.4 antes de F9.5–F9.7 (as duas mexem no `LaunchPlan`).

- [x] **F9.1 Aba Terminal.** Botão "Terminal" ao lado de Claude/OpenCode: `AgentKind::Shell` (`"shell"`) roda `bash -l` direto no tmux, na pasta do projeto, com o env do perfil e **fora do jail** (Docker, rede, `git push`). Aba e botão em âmbar; não vira `lastAgent` (o Ctrl+Shift+T continua abrindo agente). Testes: `shell_session_runs_bash_outside_the_jail`, `parse_line_knows_the_terminal_tab_and_skips_foreign_sessions`, `terminal_tab_is_a_shell_session` (tmux real) e caso no `SessionArea.test.tsx`.
  Pronto quando: um clique abre um bash no projeto sem procurar a pasta. Como testar: vitest + rota com tmux real; na VM, `docker ps` na aba.
- [x] **F9.2 Configurações do projeto no hubd.** `~/.assistente/projetos.toml` (na VM, fora de qualquer mount do jail: o agente não se dá permissão): `docker`, `allow_hosts`, `full_network`, `allowed_commands` por perfil/projeto (`hub_core::project_settings`). `GET/PUT /api/projects/{profile}/{project}/settings` (escrita atômica, 0600); aplicadas no `LaunchPlan::build_with` (rede completa = `--network` sem `--forward-port`). `[jail] docker` do `.assistente.toml` continua valendo. Testes: `project_settings::tests`, `project_settings_loosen_the_jail`, `full_network_drops_forward_ports_and_host_filter`, `settings_file_is_outside_every_jail_mount`, `project_settings_round_trip`, `project_settings_reject_bad_input`.
  Pronto quando: ligar Docker pela API faz a próxima sessão nascer com `--docker`. Como testar: testes de `LaunchPlan` + rotas com home temporário.
- [x] **F9.3 Engrenagem do projeto na UI.** Ícone na lista de projetos abre o modal `ProjectSettingsDialog.tsx` (avisos de Docker e rede completa, hosts, comandos sempre permitidos, Docker do `.assistente.toml` travado, "vale para sessões novas"). Testes: `ProjectSettingsDialog.test.tsx` + caso no `ProjectList.test.tsx`.
  Pronto quando: dá para afrouxar o jail de um projeto sem editar arquivo. Como testar: vitest com dados fake.
- [x] **F9.4 Backup inclui `projetos.toml`.** `bin/assistente backup`/`restaurar`/`vm logins` levam e trazem o arquivo (extensão da F7.1): `vm-projetos.toml` no tar, `restauracao/projetos.toml` à espera da VM, 0600 na volta. Teste: `bin/teste-backup.sh` (29 verificações).
  Pronto quando: o tar tem `vm-projetos.toml` e a restauração o devolve à VM. Como testar: `bin/teste-backup.sh`.
- [x] **F9.5 API do agente.** Listener separado `[hub] agent_listen` (127.0.0.1:7879, só `/v1/exec*`; a 7878 continua fora do jail), token por sessão no tmux (`@assistente_exec_token`), fila em memória (`crates/hubd/src/exec.rs`), execução na VM fora do jail com timeout (mata o grupo) e saída truncada; aprovação em `/api/exec`. Testes: `exec::tests` (6), `exec_token_finds_its_session_and_dies_with_it`, `exec_access_forwards_only_the_agent_port_and_hides_the_token`, `agent_listen_defaults_to_loopback_and_rejects_the_lan`, `agent_asks_user_approves_and_gets_the_output` (tmux real).
  Pronto quando: um pedido aprovado devolve saída e código; negado devolve 126. Como testar: ponta a ponta com tmux real.
- [x] **F9.6 Cliente `fora-do-jail`.** `vm/fora-do-jail` (bash + curl + python3) no jail (`~/.local/lib/assistente`, instalado pelo `provisionar.sh`) e instruções aos agentes via `configurar-agentes.sh` (bloco gerenciado no CLAUDE.md do perfil, `instructions` do opencode). Teste: `client_script_round_trip` (aprovado, negado = 126, fora de sessão, token morto, pedido perdido = 125).
  Pronto quando: o script faz o pedido, espera e repassa saída e código. Como testar: script contra o listener ao vivo.
- [x] **F9.7 Aprovar na UI.** `features/exec/ExecApprovals.tsx`: painel global (poll de 2 s) com Aprovar / Negar / Aprovar e sempre permitir neste projeto (confirma com aviso de fuga por arquivos do projeto); recentes recolhidos; bolinha na aba e contador no projeto. Testes: `ExecApprovals.test.tsx` + `hub.test.ts`.
  Pronto quando: aprovar pelo app destrava o agente. Como testar: vitest com dados fake.
- [ ] **F9.8 Validação na VM real** *(humano)*. Do jail: 7879 acessível, 7878 não; `projetos.toml`, `hub.token` e socket do tmux invisíveis; `--network`; agentes usam o `fora-do-jail` sozinhos.
  Pronto quando: os itens acima conferem. Como testar: sessão real.

**Fora da VM (no PC).** O `fora-do-jail` roda na VM, fora do sandbox, mas sem as credenciais do
PC (chave SSH do GitHub, logins). Para o que só funciona no PC (ex.: `git push`), o agente pede
e o **app** executa no host depois da sua aprovação. É a fronteira mais forte do sistema (R3):
nada roda no PC sem aprovação explícita, e a spec 05 ganha a seção "No PC" antes do código.

- [x] **F9.9 Pedido no PC.** `fora-do-jail --pc '<comando>'` manda `target: "host"` na API do agente (F9.5); o hubd guarda o alvo (`ExecTarget` em `crates/hubd/src/exec.rs`) e o mostra em `/api/exec`. Pedidos `host` nunca são auto-aprovados e o hubd nunca os executa na VM: `approve` só muda o estado para `running` (quem roda é o app, F9.10) e "sempre permitir" responde 400; sem resposta, expiram como qualquer pendente. Caminho de trabalho = pasta do projeto (o mesmo caminho no PC, via virtiofs). UI: o card mostra "no PC" e esconde "sempre permitir" (`ExecApprovals.test.tsx`); tipos em `bridge/types.ts`. Nenhuma rota nova. Testes: `exec::tests::host_requests_wait_for_the_app_and_never_run_on_the_vm` e o caso `--pc` no `client_script_round_trip`.
  Pronto quando: um pedido `--pc` aparece em `/api/exec` com `target: "host"` e o hubd nunca o roda na VM. Como testar: `exec::tests` + `client_script_round_trip` com `--pc`.
- [x] **F9.10 App executa no PC.** Comando Tauri `host_exec`: só para pedidos `host` aprovados no `ExecApprovals`, `bash -lc` na pasta do projeto (precisa estar dentro de uma raiz de perfil, como no `open_in_cursor`), com timeout e saída truncada; o resultado volta ao hubd (`POST /api/exec/{id}/result`, com o token do hub) e de lá ao agente. App fechado = o pedido espera ou expira. Feito: o app pergunta ao hubd (`GET /api/exec/{id}`) o que foi aprovado e roda exatamente aquilo (comando e pasta saem do hubd, não da UI); hubd recebe o resultado só de pedido `host` aprovado (409 nos outros) e fecha como `failed` se o app não conseguiu rodar; pedido `host` aprovado sem resultado expira depois do timeout. Testes: `api::tests::host_result_from_the_app_reaches_the_agent`, `exec::tests::host_result_is_only_accepted_for_an_approved_host_request`, `features/exec/hostExec.test.ts`, o caso no `ExecApprovals.test.tsx` e os do app (`app/src-tauri`: id do pedido, pasta do comando, timeout e saída truncada).
  Pronto quando: aprovar um `git push` pedido com `--pc` empurra do PC e o agente recebe saída e código. Como testar: vitest com hub fake + teste de rota do resultado.
- [ ] **F9.11 Cuidados do modo PC.** Card em vermelho com "vai rodar no seu PC", comando e pasta por inteiro; sem "aprovar e sempre permitir" (no máximo uma lista curta de comandos exatos por projeto, ex.: `git push`, na engrenagem F9.3, guardada no host e não na VM); registro local de tudo que rodou no PC (`~/.local/state/assistente/pc-exec.log`).
  Pronto quando: nenhum caminho aprova pedido `host` sem clique (fora da lista exata) e todo pedido executado fica no registro. Como testar: vitest do card e da lista; revisão da spec 05.
  Revisão (2026-10-22): a T3 do quadro fechou sem código (o único commit era o merge da F9.9). Já
  existe, da F9.9: nada de "sempre permitir" para `host`, na UI e no hubd (`approve` com `always`
  responde 400). Falta o resto acima e mais: o hubd marca o pedido `host` como pego pelo app (estado
  ou campo novo em `exec.rs`, devolvido só uma vez ao `host_exec`), para um segundo `host_exec` do
  mesmo pedido aprovado não rodar o comando de novo (hoje só o `busy` do `ExecApprovals` segura).
- [ ] **F9.12 Validação no PC real** *(humano)*. Do jail, `fora-do-jail --pc 'git push'` empurra com a chave do PC após aprovar; negar devolve 126; com o app fechado o pedido expira.
  Pronto quando: os itens acima conferem. Como testar: sessão real.

## F10 — Editor com diff

Ver e ajustar o que o agente mudou sem sair do app (o Cursor, F2.6, segue para trabalho
pesado). Os arquivos moram na VM: o hubd lê e grava; a UI só mostra. Tudo restrito à pasta
do projeto, dentro da raiz do perfil (mesma checagem do `open_in_cursor`). Rotas novas começam
em `not_yet("F10.x")`.

- [x] **F10.1 Arquivos e git no hubd.** `GET /api/projects/{profile}/{project}/files` (árvore, sem `.git` nem o que o `.gitignore` ignora), `GET/PUT …/files/{*path}` (texto UTF-8, recusa binário e > 1 MiB, escrita atômica, rejeita `..` e symlink que saia do projeto), `GET …/git/status` (arquivos alterados: modificado/novo/apagado/staged) e `GET …/git/diff/{*path}` (conteúdo do `HEAD` e da árvore de trabalho, para a UI montar o diff).
  Pronto quando: as rotas leem, gravam e devolvem status/diff de um repo de teste e recusam caminhos fora do projeto. Como testar: testes de rota com repo git temporário (`init_repo`).
- [x] **F10.2 Editor na UI.** Aba "Editor" ao lado dos terminais (botão junto de Claude/OpenCode/Terminal): árvore de arquivos à esquerda, editor com destaque de sintaxe (CodeMirror 6, versões com ≥ 14 dias), Ctrl+S salva pelo `PUT`, aviso ao fechar com alteração não salva e ao gravar arquivo que mudou no disco desde a abertura.
  Pronto quando: dá para abrir, editar e salvar um arquivo do projeto pelo app. Como testar: vitest com hub fake.
- [x] **F10.3 Diff do git.** Lista de alterados (do `git/status`) com contador na aba; clicar abre o diff `HEAD` × árvore de trabalho (lado a lado, com opção inline, via `@codemirror/merge`), editável do lado direito; botão para descartar a alteração de um bloco ou do arquivo inteiro (confirma antes).
  Pronto quando: depois de uma sessão de agente, a lista mostra o que mudou e o diff de cada arquivo. Como testar: vitest com hub fake + rota com repo temporário.
- [x] **F10.4 Diff da manhã.** Na tela "Bom dia" (F6.6), cada commit da noite abre no mesmo visualizador de diff (`GET …/git/diff` com `from`/`to` de commits).
  Pronto quando: dá para revisar o trabalho noturno arquivo a arquivo antes de mesclar. Como testar: vitest com dados fake.

## F11 — Kanban dos agentes

Quadro por projeto (backlog · fazendo · feito): você escreve as tarefas no app e as sessões
de agente e o modo dormir (R6) **puxam a próxima sozinhos**, em vez de procurar o que fazer
à mão. Fonte da verdade: `.assistente/kanban.json` versionado no repo do projeto — o hubd lê
e grava (e comita sozinho, para a árvore não amanhecer suja e travar o dormir, F6.3), e o
clone do notebook leva o quadro junto, então o que os agentes marcaram à noite volta no
merge da manhã (F6.5) e os commits dos movimentos aparecem na revisão de diffs (F10.4). O
`docs/FUNCIONALIDADES.md` continua sendo o backlog deste próprio assistente; o quadro é por
projeto. Rotas novas em `not_yet("F11.x")`. Ordem: F11.1 antes dos demais (todos falam com o
hubd); F11.2 antes de F11.4.

- [x] **F11.1 Quadro no hubd.** `GET /api/projects/{perfil}/{projeto}/kanban` (tarefas por coluna: título, descrição, prioridade, sessão que segura), `POST` (criar), `PATCH …/kanban/{id}` (título/descrição/prioridade/coluna) e `DELETE …/kanban/{id}`. Toda escrita grava o JSON e comita (mensagem própria, tipo `kanban: move T3 para fazendo`). Tarefa em "fazendo" fica presa à sessão: morreu a sessão (a lista do tmux é a fonte, F2.2), a tarefa volta para o topo do backlog sem perder o texto.
  Pronto quando: criar, mover e excluir pela API deixa um commit no histórico do repo; sessão morta devolve a tarefa ao backlog.
  Como testar: testes de rota com repo git temporário (`init_repo`, como na F10.1). Feito: `api::kanban::tests` (CRUD com um commit por escrita, sessão morta solta a tarefa) e `hub_core::kanban::tests`. O quadro tem dois tipos (`manual` padrão, que só roda com "Mandar"; `agente`, que o orquestrador e o modo dormir puxam). Cada commit leva só `.assistente/kanban.json`.
- [x] **F11.2 Puxa a próxima (API do agente).** `POST /v1/kanban/pull` e `POST /v1/kanban/done` no listener do agente (F9.5, porta 7879, token por sessão): `pull` move a primeira tarefa do backlog (ordem por prioridade, depois criação) para "fazendo" com a marca da sessão, atomicamente — duas sessões simultâneas nunca levam a mesma; `done` move para "feito" com referência ao commit. Quadro sem tarefa = resposta "vazia": o agente fica parado, não inventa trabalho. Cliente `fora-do-jail kanban` (F9.6) e bloco nas instruções do perfil (`configurar-agentes.sh`, F5.2): "termine a tarefa atual antes de puxar a próxima".
  Pronto quando: duas sessões simultâneas num quadro com 2 tarefas levam tarefas distintas, e uma terceira chamada responde "vazia".
  Como testar: testes do listener com duas sessões fake e um quadro de 2 tarefas. Feito: `api::kanban::tests::only_the_holder_finishes_or_releases_and_empty_board_is_null` e `pull_gives_agent_cards_…` (sessões de tmux de verdade); `fora-do-jail kanban pull|done|release|trabalhador`.
- [x] **F11.3 Quadro na UI.** Botão "Quadro" na área de sessões (ao lado de Claude/OpenCode/Terminal/Editor, F10.2): três colunas, card arrastável (mudar de coluna = `PATCH`), criar/editar/apagar num modal (padrão ProjectSettingsDialog), contador de pendências no item do projeto da lateral e nome da aba responsável no card. Poll curto (padrão do ExecApprovals, F9.7).
  Pronto quando: uma sessão de agente puxa uma tarefa e o card aparece em "fazendo" com o nome da aba, sem recarregar a UI.
  Como testar: vitest com hub fake + caso em `SessionArea.test.tsx`. Feito: `features/kanban/KanbanBoard.test.tsx` (5 casos) e `ProjectList.test.tsx` (contador do quadro). O nome da aba responsável aparece no card ("em Claude 1").
- [x] **F11.4 Dormir lê o quadro.** Cada iteração do `dorme` (F6.3) começa com `pull`: o prompt da iteração traz título e descrição da tarefa (o resto do briefing continua); ao terminar, o agente roda `kanban done`. Se a iteração não gerou commit, a tarefa volta para o backlog e o log explica. Quadro vazio = o `dorme` dorme sem iterar. No "Bom dia" (F6.6), a contagem aparece por tarefa: qual tarefa gerou quais commits.
  Pronto quando: uma noite com 2 tarefas no quadro = 2 iterações, 2 commits e as duas em "feito" no quadro visto pelo app.
  Como testar: `notebook/teste-dorme.sh` com quadro fake de 2 tarefas + o guard de WIP (F6.3). Feito: opt-in por projeto com `[sleep] source = "quadro"` (o padrão continua `backlog`). O `dorme` puxa, marca "feito" com o commit ou devolve ao backlog. "Bom dia" por tarefa ainda não: a contagem por card fica para depois.
- [x] **F11.6 Dois tipos de card e trabalhadores.** `manual` (padrão) nunca roda sozinho: só com
  "Mandar" no card, e o trabalhador abre na hora. `agente` é puxado pelo orquestrador (F11.7) ou pelo
  modo dormir (F11.4). Cada execução roda `vm/trabalhador` no jail, com o opencode, dentro de um
  worktree próprio (`.assistente/worktrees/<id>`, branch `agente/<id>`, fora do `git status`). Tenta
  cada modelo de `[workers] models` (principal primeiro, depois reservas) `attempts` vezes; progresso =
  commit novo com a árvore limpa; sucesso marca "feito" com o commit; esgotou tudo, devolve ao backlog.
  Rotas: `POST …/kanban/{id}/mandar`, `POST /api/sessions/{id}/reiniciar`.
  Pronto quando: "Mandar" abre um trabalhador no worktree, o card mostra quem segura, e um modelo que
  falha passa para a reserva sozinho. Como testar: `vm/teste-trabalhador.sh` (opencode falso: reserva,
  esgotar, árvore suja) e `api::kanban::tests::mandar_opens_a_worker_on_a_worktree_and_restart_keeps_the_card`.
- [x] **F11.7 Orquestrador e supervisão.** A sessão do Claude é o orquestrador: `fora-do-jail kanban
  pull` pega uma tarefa `agente`, escolhe o modelo e abre o trabalhador com `fora-do-jail kanban
  trabalhador <id> --modelos principal,reserva…` (`POST /v1/workers`, só sessão do Claude; trabalhador
  não abre outro). Quem cuida: o `trabalhador` troca de modelo e tenta de novo sozinho; o hubd solta a
  tarefa quando a sessão morre (na próxima leitura do quadro) e "Reiniciar" relança o trabalhador com o
  mesmo modelo, no mesmo worktree, passando a tarefa para a sessão nova. Instruções em
  `vm/configurar-agentes.sh` (bloco do CLAUDE.md).
  Pronto quando: o orquestrador puxa, abre o trabalhador e a tarefa passa para ele; um trabalhador não
  abre outro; reiniciar mantém a tarefa. Como testar: `api::kanban::tests::orchestrator_hands_the_card_to_a_worker_and_workers_stop_there`
  e `…mandar_opens_a_worker…` (reiniciar).
- [ ] **F11.5 Validação na VM real** *(humano)*. Quadro com 3 tarefas e 2 sessões abertas: cada sessão puxa uma distinta; mover um card pelo app; fechar uma das abas e ver a tarefa voltar ao backlog; `dorme` de noite alimentado pelo quadro, com revisão no "Bom dia".
  Pronto quando: os itens acima conferem. Como testar: sessão real.
- [x] **F11.9 Popular o quadro pelo backlog.** Botão "Popular pelo backlog" no quadro abre um Claude
  que lê os `.md` do projeto (itens `- [ ]` de `docs/FUNCIONALIDADES.md` e parecidos, pulando os
  `*(humano)*` e os `[x]`), confere o que já está no quadro com `fora-do-jail kanban listar` e cria os
  cards com `fora-do-jail kanban criar --titulo … --descricao … --prioridade …`. Os cards nascem `manual`:
  você marca os que vão para o agente. Não implementa nada.
  Pronto quando: o botão abre a sessão, e os cards aparecem no quadro sem duplicar. Como testar:
  `api::kanban::tests::explorar_opens_a_claude…`, `…agents_list_and_create_cards…`,
  `…fora_do_jail_kanban_talks_to_the_board` (roda o script de verdade) e `session::explore_starts…`.
- [ ] **F11.8 Validação na VM real** *(humano)*. Um card `manual` com "Mandar" e um `agente` puxado pelo
  orquestrador, com o opencode de verdade: o modelo grátis falha e a reserva assume; "Reiniciar" num
  trabalhador travado; `opencode models` confere o formato e os grátis que o F11.10 escolhe (e os nomes
  que vão em `[workers] models`, se usar override). Confirme também
  se o `opencode run --standalone` roda dentro do jail (o trabalhador passa a usar a flag: sem ela a
  porta 49374, do ai-memory, fica ocupada e toda tentativa aborta). Visto na VM (2026-10-21): com a
  flag o `run` sobe e chega ao provedor; o catálogo marca vários grátis como `deprecated` (agora
  filtrados) e o provedor pode responder `Model unavailable` (agora troca de modelo na hora). Falta
  uma tarefa concluída de ponta a ponta.
  Pronto quando: os itens acima conferem. Como testar: sessão real.
- [x] **F11.10 Modelo automático.** Sem `[workers] models` e sem `--modelos`, o hubd lista os modelos
  do catálogo do opencode (`~/.cache/opencode/models.json`), fica com os do provedor `opencode` de
  custo zero e ordena pela taxa de sucesso do perfil (suavizada; empate = ordem do catálogo). Cada tentativa
  grava `ok` ou `falhou` em `trabalhadores/<sessão>.modelos`, e o histórico soma todas as execuções.
  O `trabalhador` vigia a saída do opencode: erro de provedor (rate limit, 429/5xx, modelo inexistente,
  conexão caída) mata a tentativa na hora e troca de modelo. `[workers] models` continua valendo como
  override. Pronto quando: "Mandar" sem configuração escolhe um grátis e um erro de provedor troca de
  modelo sem esperar a tentativa acabar. Como testar: `api::kanban::tests::mandar_without_models…`,
  `worker::tests::{free_models…,ranking…,outcomes…,read_outcomes…}` e `vm/teste-trabalhador.sh`
  (caso do provedor recusado, com modelo falso preso). Verificado na VM (2026-10-21): `opencode models`
  não lista nada no hubd (a porta do serviço, 49374, é do ai-memory; `--standalone` roda sem a config
  do perfil e sai vazio), então a escolha lê o catálogo `~/.cache/opencode/models.json`, que tem o
  provedor `opencode` com `cost`. Falta confirmar na VM que os grátis rodam no `opencode run` do jail.

- [x] **F11.11 Orquestrador automático.** Botão "Orquestrador" no quadro (com o limite de 1 a 4
  tarefas em "fazendo"): ligado, o hubd puxa sozinho as tarefas `agente` do backlog, a cada 20 s, e
  abre um trabalhador para cada uma pelo mesmo caminho do "Mandar", sem sessão do Claude. Card que
  volta do "fazendo" sem concluir (trabalhador desistiu, "Parar", sessão morta) conta `failures` e o
  automático não puxa de novo até "Tentar de novo". Erro do laço aparece no quadro. Ligar, desligar
  e mudar o limite avisam o resultado (o que abriu ou por que não). Instruções extras, somadas às
  regras padrão do prompt de todo trabalhador do projeto, num diálogo ao lado. Rota `PUT
  …/kanban/orquestrador`. Pronto quando: ligar abre trabalhadores até o limite, uma vaga liberada é
  preenchida pela próxima, e o card que falhou fica no backlog. Como testar:
  `api::kanban::tests::orchestrator_fills_up_to_the_limit…`, `…orchestrator_error_shows_on_the_board`,
  `kanban::tests::auto_skips…` e `KanbanBoard.test.tsx`. Na VM real, junto com F11.8.
- [ ] **F11.12 Revisar e juntar o que os trabalhadores fizeram.** Hoje um card "feito" só tem commit
  na branch `agente/<id>`: nada chega à `main` sem uma conversa à mão (revisar cada branch, juntar,
  resolver conflito, rodar o check, devolver o que ficou ruim). Botão "Revisar" no quadro abre um
  Claude (como o "Popular pelo backlog", F11.9) com um prompt de revisão; o orquestrador (F11.11)
  chama o mesmo sozinho quando não há nada em "fazendo" e há card "feito" fora da `main`. O revisor:
  lê o diff de cada branch contra a `main` e confere com o "Pronto quando" do card; junta numa
  worktree descartável, em ordem de dependência (F11.13), resolvendo os conflitos e corrigindo só o
  que **nasce da junção** (dois arquivos novos para a mesma coisa, código que um card esperava do
  outro); roda `./dev just check` nela (a sessão sobe com `--docker`); e pede ao hubd para andar com
  a `main` pela regra da F8.8 (fast-forward se ela é o checkout, senão troca a ref conferindo o valor
  antigo; nunca push). Defeito de uma branch sozinha não é corrigido ali: o card volta para o
  backlog com um parágrafo "Revisão (data)" dizendo o que já existe e o que falta (`fora-do-jail
  kanban reabrir <id> --nota …`), e o item do `docs/FUNCIONALIDADES.md` (quando existe) ganha a mesma
  nota; defeito que não é de card nenhum vira item novo. Card juntado ganha `merged` (o commit na
  `main`) e o selo "na main"; worktree e branch saem (`git branch -d`). O resumo (o que entrou, o que
  voltou e por quê) aparece no quadro e fica em `.assistente/revisoes/<data-hora>.md`, fora do git.
  Check vermelho ou `main` movida: a `main` não muda e o resumo diz onde parou.
  Pronto quando: com 3 cards feitos (dois que se cruzam e um sem código próprio), um clique em
  "Revisar" leva os dois para a `main` com o check verde, devolve o terceiro ao backlog com a nota e
  apaga os worktrees, sem conversa à mão. Como testar: `api::kanban::tests` com repo git real
  (`init_repo`): sessão do revisor aberta com o prompt, `reabrir` com nota, `merged`, `main` movida =
  nada muda; `KanbanBoard.test.tsx` (botão, selo, resumo).
- [ ] **F11.13 Cards com dependência e progresso de verdade.** O card ganha `depends_on` (ids). O
  "Popular pelo backlog" (F11.9) preenche pela "Ordem:" das seções e pelos "Depende de" dos itens, e
  a engrenagem do card deixa editar. O orquestrador e o "Mandar" não abrem trabalhador para card
  cuja dependência ainda não está na `main` (F11.12): o worktree nasce do `HEAD`, então já sai com o
  código dela. Na revisão de 2026-10-22, F12.2–F12.4 rodaram em paralelo sem o código uma da outra
  e saíram três caixas de pedido diferentes. E o `trabalhador` só conta como progresso commit com
  mudança própria: um merge sem diff próprio (a T3 da mesma revisão) não fecha o card.
  Pronto quando: um quadro com A → B abre só A, e B abre depois que A entra na `main`, num worktree
  com o código de A; um trabalhador que só faz merge volta ao backlog. Como testar:
  `kanban::tests` (a fila respeita `depends_on`), `api::kanban::tests::orchestrator_…` e
  `vm/teste-trabalhador.sh` (merge sem diff não é progresso).

## F12 — Apontar e pedir na tela

Enquanto o sistema roda, aponte para o que quer mudar. O app abre o projeto numa webview filha
(o mesmo padrão do painel da memória, F5.5) com um modo de seleção: o elemento sob o mouse ganha
contorno, o clique o congela e abre a caixa de pedido; o agente recebe componente,
`arquivo:linha` (quando o build expõe), seletor, texto e o seu recado, edita o código e o hot
reload mostra o resultado na própria webview. O pedido vai para a sessão que está trabalhando no
projeto ou vira card no quadro (F11). Depende de F9.2 (URL do preview por projeto) e, para o
quadro, de F11.1–F11.3. Rotas novas em `not_yet("F12.x")`.

O dev server precisa ser alcançável de fora do jail: a webview carrega a URL direto, então escute
na interface da VM (ex.: `vite --host 0.0.0.0`) e rode-o na aba Terminal (F9.1, fora do jail) ou
ligue rede completa (F9.2) — com a rede filtrada, o loopback é só do sandbox. A URL fica em
`~/.assistente/projetos.toml` (só `http`/`https`, sem credenciais), nunca no repo. Como no painel
da memória, a webview do preview fica fora das capabilities: a página apontada não alcança os
comandos do app.

- [x] **F12.1 Preview do projeto.** `preview_url` opcional em `ProjectSettings` (F9.2) e no espelho `app/src/bridge/types.ts`, editável na engrenagem (F9.3), com a dica de que o endereço é visto do PC (projeto rodando na VM usa o IP dela). Comandos Tauri no molde de F5.5: `open_preview_panel`, `update_preview_panel_bounds`, `preview_panel_visible` (esconde ao abrir o modal), `preview_picker` (liga/desliga o modo por `eval` no script) e `close_preview_panel`, validando `http`/`https` e recusando credenciais. Botão "Visualizar" junto dos agentes; o chrome (URL, recarregar, apontar, fechar) é React e a webview preenche o retângulo, inclusive ao redimensionar a janela. Testes: `project_settings_round_trip` com `preview_url`, validação da URL e caso em `ProjectSettingsDialog.test.tsx`.
  Pronto quando: colar a URL do dev server na engrenagem e clicar em Visualizar abre o app dentro do Assistente, sem fechar a sessão.
  Como testar: `./dev just hubd` + `vite --host` de um projeto de teste e o painel no app.
  Feito: `preview_url` no `ProjectSettings` do hub-core (validado no `PUT`, sem credenciais) e no espelho da UI; aba "Preview" no projeto (botão "Visualizar" junto dos agentes, com a sessão viva); webview filha `preview` ancorada como a da memória, fora das capabilities; o modal da engrenagem esconde a webview. O modo apontar tem o comando e o gancho na página; a seleção é da F12.2.
- [x] **F12.2 Modo apontar.** Script injetado na webview do preview (`initialization_script`, fora do bundle do projeto): botão flutuante "Apontar", contorno no elemento sob o mouse, clique capturado na fase de captura (sem disparar o clique da página), Esc cancela e um novo clique troca a seleção. O pick leva tag, id, classes, `role`/`aria`, texto curto, trecho de HTML curto, seletor CSS estável, caminho no DOM, retângulo, viewport/scroll e URL; chega ao app por uma navegação `assistente-pick:` interceptada com `on_navigation` no Rust, que descarta payload acima do limite, anexa a URL real da webview (não confia na URL do payload), emite o evento e cancela a navegação — sem IPC e sem CORS na página remota. Testes: parser do pick e interceptação em Rust; `PickDialog.test.tsx` com pick fake; manual com app de teste.
  Pronto quando: apontar um botão no app rodando abre a caixa de pedido com o elemento certo, e o botão da página não dispara a ação.
  Como testar: app de teste com um botão que muda de cor; apontar e conferir que nada mudou antes de enviar.
  Feito: `app/src-tauri/src/pick.js` (injetado pelo plugin `pick` em toda webview; fica de fora a
  `main` e a `memoria`) e `pick.rs` (parser, limite de 64 KiB, URL real, evento `preview-pick`). O
  `PreviewPanel` escuta o evento e abre a caixa do F12.4 (`features/pick/detail.ts` converte o pick),
  esconde a webview enquanto ela está aberta, e o botão de apontar da barra liga o script por
  `setEnabled`. Testes: `pick::tests`, `pickScript.test.ts`, `detail.test.ts` e `PreviewPanel.test.tsx`.
- [ ] **F12.3 Contexto de código.** No pick, subir pelo fiber do React (`__reactFiber$…`) para colher a pilha de componentes e `_debugSource` (`arquivo:linha`, presente no dev com Vite/React), mapeando `/src/...` para o caminho do projeto. Sem fonte (build de produção, inclusive a UI servida pelo hubd na F8.3), o pick leva componente, seletor e texto, e o prompt manda o agente procurar antes de editar (grep pelo texto/classe/componente) em vez de chutar. Testes: extração com DOM fake (jsdom) cobrindo fiber com e sem `_debugSource` e página sem React; caso no `PickDialog`.
  Pronto quando: apontar um botão em dev mostra `src/features/x/Botao.tsx:42` no pedido; no build de produção mostra o componente e o agente acha o arquivo pelo texto.
  Como testar: vitest com DOM fake + um build de produção.
  Revisão (2026-10-22): o código existe e tem testes, mas não está ligado. `codeContext.js` (sobe
  pelo fiber; `_debugSource` até o React 18, `_debugStack` no 19), `context.ts` (`toProjectPath`,
  aviso de busca), `prompt.ts` e `PickCodeContext.tsx`, todos em `app/src/features/pick/`. Falta:
  injetar `codeContext.js` antes do `pick.js` (no `SCRIPT` do `pick.rs`); o `pick.js` mandar
  `code: window.__assistenteCodeContext.extract(element)`; `PickedElement` (TS e `pick.rs`) ganhar
  `code`; o `toPickDetail` preencher `source` (`toProjectPath`, com a raiz do projeto),
  `component` e `components`; a caixa do F12.4 mostrar o `PickCodeContext`. E arrumar o que sobrou
  da T6: um tipo de pick só (hoje `PickedElement`, `PickElement` de `pick/types.ts` e `PickDetail`)
  e um prompt só (o do hubd, `hub_core::pick`; `prompt.ts` sai, e o aviso de busca do hubd usa o
  texto/classe/componente como o `searchInstruction`).
- [x] **F12.4 Enviar ao agente.** `POST /api/projects/{perfil}/{projeto}/picks`: escreve `.assistente/picks/<data-hora>.md` com o detalhe do pick (fora do git via `.git/info/exclude`, como o `.ai-jail` da F0.5), monta o prompt curto (URL, onde, seletor, texto, pedido) e entrega no alvo escolhido no modal: **sessão** (só de agente, nunca a aba Terminal; confere que é do projeto e está viva, cola no prompt com `set-buffer` + `paste-buffer` **sem Enter** — Enter é seu, para não aprovar pergunta pendente nem executar texto no shell) ou **quadro** (card no backlog da F11.1, autossuficiente: o clone do notebook não tem o arquivo local). O modal mostra o prompt que será enviado, deixa editar o texto e escolher a sessão (padrão: a aba ativa do projeto); ao colar, a UI leva para a aba e avisa "confirme no terminal". Testes: rota com tmux real (colar e ler de volta, sem o pedido rodar sozinho), card fake, guards (texto vazio, sessão de outro projeto ou tipo shell, sessão/projeto inexistente), `PickDialog.test.tsx` (envio, erro, troca de alvo).
  Pronto quando: apontar um botão, escrever "deixa verde" e enviar faz o texto aparecer no prompt da sessão; depois do seu Enter, o agente edita o arquivo certo.
  Como testar: rota com tmux real + manual.
  Feito: `crates/hubd/src/api/picks.rs` + `hub_core::pick` (prompt, card e arquivo) e a caixa em
  `features/pick/PickDialog.tsx`, aberta pelo pick da F12.2 (ligada no merge da revisão de
  2026-10-22: a T7 tinha a caixa, mas nada a abria). Sessão com painel morto (F14.3) responde 409.
- [x] **F12.6 O preview não cobre o app.** A webview do preview é um widget nativo por cima de toda a
  UI: hoje ela só some com a engrenagem e com a caixa do apontar. Ficam por baixo dela o painel de
  pedidos (`ExecApprovals`, no canto inferior direito, inclusive os pedidos para o PC da F9.10), o
  `Toast`, o "Bom dia" (F6.6) e o painel do Assistente (F8). Esconder a webview enquanto qualquer
  sobreposição estiver aberta (um seletor só no store, usado pelo `PreviewPanel`), ou reservar o
  canto dos pedidos fora do retângulo dela. O painel da memória (F5.5) tem o mesmo padrão: conferir.
  Pronto quando: com o preview aberto, um pedido novo do agente aparece e dá para aprovar sem fechar
  o preview. Como testar: `PreviewPanel.test.tsx` (pedido pendente esconde a webview; sem pedido,
  ela volta).
- [ ] **F12.5 Validação na VM real** *(humano)*. Dev server na aba Terminal (F9.1), preview aberto e dois apontamentos (um botão e um texto); enviar para a sessão e conferir o hot reload depois do Enter; repetir com o agente ocupado e com a sessão fechada (erro claro + card no quadro); build de produção sem `_debugSource`; conferir que a página do preview não chama comando do app (nenhuma capability aponta para a origem dela).
  Pronto quando: o ciclo apontar → descrever → confirmar no terminal → ver a mudança leva menos de um minuto e a webview do preview continua sem comandos do app.
  Como testar: manual *(humano)*.

## F13 — Git remoto

- [ ] **F13.1 Push para o GitHub a partir deste PC** *(humano)*. `git push origin main` falhou com
  "Host key verification failed": o PC não tem chave SSH (`~/.ssh` inexistente) nem `known_hosts` com
  o GitHub, e o `gh` não está instalado. Escolher um dos caminhos: (a) chave SSH na conta do GitHub +
  aceitar a chave do host (`ssh -T git@github.com`); ou (b) `gh auth login` e trocar o remote para
  HTTPS. Não aceitar a chave do host sem conferir a impressão digital.
  Pronto quando: `git push origin main` envia os commits pendentes da `main` (`b7c80ca`, `b1234fa`).
  Como testar: `git push --dry-run origin main` sem erro.

## F14 — Diagnóstico da VM pelo agente

- [x] **F14.1 Leitura de diagnóstico sem aprovação.** Rota nova na API dos agentes (`127.0.0.1:7879`,
  token da sessão, como o `fora-do-jail`): `POST /v1/diagnostico` com `{"comando": "..."}`. Só aceita
  uma lista fixa de comandos de leitura, escrita no Rust (não é shell livre): `journal-hubd` (últimas N
  linhas de `journalctl -u hubd`), `tmux-ls`, `tmux-pane` (`capture-pane` de uma sessão do projeto,
  com `-S -200`), `explorar` (lista e tamanho dos prompts em `explorar/`) e `claude-login`
  (`bash -l -c 'command -v claude'`). A saída é cortada em 64 KiB e comando fora da lista responde 400.
  Não passa pelo `ExecApprovals` (F9.7) porque não escreve nada. Testes: parser e recusa de comando
  fora da lista em Rust; chamada com token errado responde 401.
  Pronto quando: um agente dentro de uma sessão do projeto lê o journal do hubd e a saída de uma sessão
  morta sem pedir aprovação. Como testar: `cargo test` das rotas + uma sessão de agente que chama a rota.
  Feito: `crates/hubd/src/api/diagnostico.rs` (lista fechada, saída cortada em 64 KiB, painel só de
  sessão do próprio projeto) + rota na `agent_router`; o prompt da sessão que explora o backlog passa
  a ser gravado em `~/.assistente/perfis/<perfil>/explorar/<sessão>.md` (como a spec 10 promete), para
  o `explorar` ter o que listar; cliente `fora-do-jail diagnostico <comando>` e instruções aos agentes
  (`vm/configurar-agentes.sh`). Testes: `api::diagnostico::tests` (parser, corte), `api::tests::
  diagnostico_le_sem_aprovacao_e_recusa_o_que_esta_fora_da_lista` (401, 400, painel de outro projeto) e
  `cliente_diagnostico_devolve_a_saida_sem_pedido_de_aprovacao` (o cliente de verdade).
- [x] **F14.2 Journal do hubd sem sudo.** `vm/provisionar.sh` põe o usuário no grupo `systemd-journal`,
  para `journalctl -u hubd` rodar sem senha de `sudo` (a VM tem a senha travada, `lock_passwd: true`).
  Vale depois de relogar. Idempotente: rodar `provisionar.sh` duas vezes não falha. Feito: `usermod -aG`
  no `provisionar.sh` (avisa e segue se o grupo não existir ou não entrar na conta); o restart do hubd
  logo abaixo já roda com o grupo novo, que é o que a rota de diagnóstico (F14.1) usa.
  Falta: conferir na VM (manual, `groups` e `journalctl -u hubd -n 5`).
  Pronto quando: `groups` mostra `systemd-journal` e `journalctl -u hubd -n 5` lê as linhas sem `sudo`.
  Como testar: `bash -n vm/provisionar.sh` e `shellcheck`; na VM, `groups` e `journalctl -u hubd -n 5`.
- [x] **F14.3 Sessão que morre deixa o painel.** Sessões de agente sobem com `remain-on-exit` no tmux
  (só o painel da sessão, não o servidor), para a saída de quem saiu com erro continuar visível na aba
  até ela ser encerrada. A aba mostra "Sessão encerrada (status N)" e o que o processo imprimiu. Hoje
  a aba só mostra o terminal vazio, e foi assim que `claude: not found` passou despercebido.
  Pronto quando: uma sessão que sai com erro mostra a última saída e o status na aba.
  Como testar: rota com tmux real (um comando que sai com status 127 e `capture-pane` com o erro);
  `TerminalView.test.tsx` com a mensagem de encerramento.
  Feito: o hubd liga o `remain-on-exit` na janela da sessão (encadeado no mesmo `tmux` do
  `new-session`, sem corrida) e avisa a UI com `{"type":"exit","status":N}`; se a sessão já
  estava morta, manda a saída do painel (o tmux perde a primeira linha da tela quando o
  processo morre) em vez de abrir terminal. Sessão morta não segura tarefa do quadro nem
  token da API dos agentes. O status de saída não depende do tmux: ele perde o `SIGCHLD` do
  painel morto em ~5% das vezes (o processo fica zumbi e o `pane_dead_status` não vem), e um
  `run-shell` faz o servidor recolher o zumbi (medido: 15 de 15 casos recuperados). Sem
  código de saída (processo morto por sinal), a aba avisa só que a sessão acabou. Testes:
  `command_that_exits_at_once_leaves_the_dead_pane`,
  `exec_token_of_a_dead_session_is_refused`, `pane_state_knows_the_status_of_a_dead_pane`,
  `pane_state_tells_alive_from_dead`, `dead_session_shows_the_output_and_the_exit_status`,
  `session_that_dies_with_the_tab_open_sends_the_status` e `TerminalView.test.tsx`.
- [ ] **F14.4 Sessão morta não fica para sempre.** O `remain-on-exit` da F14.3 vale para toda sessão do
  `Tmux::create`, não só para os agentes: o trabalhador (F11.6) que termina e a aba Terminal (F9.1)
  depois do `exit` também ficam no tmux, mortos, até alguém encerrar. Com o orquestrador ligado
  (F11.11), cada tarefa concluída deixa uma aba morta. Ligar o `remain-on-exit` só no Claude e no
  OpenCode interativos; o trabalhador que termina bem fecha a própria sessão (o que falha fica, para
  ver o erro); a aba Terminal fecha no `exit`. `SessionInfo` ganha `dead` (o `Listed` já sabe) para a
  UI marcar a aba morta sem abrir o terminal. Cada terminal aberto pergunta ao tmux a cada 500 ms se
  o painel morreu (`terminal.rs`): ver se dá para usar o evento `pane-died` do tmux no lugar.
  Pronto quando: o orquestrador com 2 tarefas concluídas não deixa sessão morta, e um Claude que sai
  com erro continua mostrando a saída e o status. Como testar: rota com tmux real (trabalhador falso
  que sai 0 some; Claude que sai 127 fica) + `SessionArea.test.tsx`.

## Depois

- Notificação no celular (ntfy) quando o `dorme` termina ou escreve RELATORIO.md.
- Acompanhar o modo dormir pelo celular (UI web servida pelo hubd através do Tailscale).
- Mais agentes (codex, crush) — o `AgentKind` e o ai-jail já são genéricos.
- Memória no notebook só como réplica de longa duração se o PC ficar meses off (fora do fluxo normal F5.4).
- Duas VMs (pessoal/trabalho) se o trabalho exigir isolamento total.
