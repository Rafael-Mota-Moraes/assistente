# 09 — Auto-atualização (aba Assistente)

Cobre R10. Funcionalidades F8.1–F8.7.

Uma aba fixa **Assistente** abre um agente no próprio repositório do Assistente. O que ele
commita entra em uso com um clique, sem fechar o app e sem derrubar as sessões abertas,
inclusive a da própria aba.

## O que muda e como entra no ar

| Camada | Código | Como atualiza sem fechar | Sessões tmux |
|---|---|---|---|
| UI | `app/src` | o hubd serve o build da UI; o app recarrega a página | continuam; os terminais reconectam |
| hubd | `crates/` | binário novo + restart do serviço (`KillMode=process`, F1.4) | continuam |
| Casca Tauri | `app/src-tauri` | **não atualiza sozinha**: precisa de `assistente atualizar` e reabrir o app | continuam |

A maior parte das mudanças é na UI e no hubd. A casca é fina de propósito (spec 03) e muda pouco.

## Fluxo

1. **Aba Assistente.** Botão fixo no topo da lateral. Abre (ou reaproveita) uma sessão Claude
   no projeto configurado em `[self_update]` (abaixo). É uma sessão normal (tmux → ai-jail →
   claude); o que muda é o lugar na UI e o painel de atualização ao lado do terminal.
2. **O agente trabalha e commita.** Só entra em uso o que está commitado. A árvore de
   trabalho do agente nunca é compilada.
3. **Painel de atualização.** Mostra o commit em uso (UI e hubd), o HEAD do repo, os commits
   pendentes e as camadas que eles tocam (pelos caminhos alterados). Commits que tocam
   `app/src-tauri` ganham o aviso "precisa reinstalar o app".
4. **Aplicar** (`POST /api/update/apply {commit}`): o hubd dispara o atualizador (abaixo) e o
   painel acompanha o log por `GET /api/update`.
5. **Recarregar.** Quando o commit da UI em uso muda, o app avisa ("versão nova aplicada") e
   recarrega a página. Perfil, projeto e aba ativa voltam do `sessionStorage` (F8.2), e os
   terminais reconectam ao tmux (F8.1).

```toml
# assistente.toml
[self_update]
profile = "pessoal"
project = "assistente"   # pasta dentro da raiz do perfil
```

## Atualizador (`vm/atualizar.sh`, F8.5)

O hubd o lança desanexado (`setsid`), como o tmux, então ele sobrevive ao restart do hubd.
Roda como o seu usuário na VM, **fora do ai-jail**, e grava estado e log em
`~/.assistente/atualizacoes/<commit>/{estado.json,log}`.

1. `git worktree add` limpo do commit pedido em `~/.assistente/atualizacoes/<commit>/src`.
2. `./dev just check` no worktree, usando o Docker da VM. Se falhar, para aqui e nada muda.
3. `./dev just build-ui` (vite) e `./dev just build-hubd`, só das camadas que mudaram.
4. Instala em `~/.assistente/versoes/<commit>/{hubd,ui/}` e troca os links `atual` e
   `anterior`.
5. Se o hubd mudou: reinicia o hubd e espera `GET /api/health` responder com o commit novo em
   até 30 s. Se não responder, volta `atual` para `anterior`, reinicia de novo e marca falha.
6. Apaga o worktree e mantém as 3 últimas versões — sem tocar nos alvos dos links `atual` e
   `anterior` (a poda por data poderia apagar justamente a versão do "Voltar").

`vm/teste-atualizar.sh` (`./dev just teste-atualizar`) reproduz o ciclo no devenv com um
repositório e um serviço falsos: aplica, guarda `anterior`, saúde quebrada volta sozinha,
verificação que falha não muda nada e a poda respeita os links. Os comandos de reinício e de
saúde são configuráveis por env (`ASSISTENTE_RESTART_CMD`, `ASSISTENTE_HEALTH_TRIES`,
`ASSISTENTE_HEALTH_WAIT`), para o teste trocar o `systemctl` por um serviço falso.

`vm/hubd.service` passa a `ExecStart=%h/.assistente/versoes/atual/hubd` com
`Restart=always`: o atualizador encerra o hubd com SIGTERM, e o `on-failure` de hoje não
reiniciaria depois disso. O provisionamento instala a primeira versão nesse mesmo lugar.

**Voltar** (`POST /api/update/rollback`) troca `atual` por `anterior` e reinicia o que
mudou. Fica no painel e também na UI embutida, para quando a UI nova não carregar.

## UI servida pelo hubd (F8.3, F8.4)

- O hubd serve `GET /ui/*` de `~/.assistente/versoes/atual/ui`, sem token (o bundle não tem
  segredo). `GET /api/health` passa a devolver também `commit`, o da UI e o do hubd.
- A UI embutida no binário (`frontendDist`) vira **carregadora e reserva**. Ao abrir, ela
  confere se o hubd tem UI compatível e leva a janela para `http://<vm.ip>:<porta>/ui/`. Se o
  hubd estiver fora, sem UI, ou a página não sinalizar "pronta" em 10 s, a reserva continua
  na tela e mostra o erro e o botão Voltar.
- A página remota usa os comandos de host e os plugins pela capability `remote.urls` do
  Tauri 2 (`http://<vm.ip>:<porta>/*`), de novo amarrada ao IP e à porta da VM, como o escopo
  do plugin HTTP.
- **Compatibilidade com a casca:** a casca expõe `host_api_version`, e a UI declara a mínima
  de que precisa. Casca antiga mostra a faixa "reinstale o app (`assistente atualizar`)" e a
  UI desliga o que depende de comando novo.
- Bônus: a página vem de `http://`, então o fetch para o hubd é da mesma origem e some o
  problema de mixed content da spec 03 (os plugins continuam, por causa da reserva).

## Segurança

- **Só aplica com clique humano.** Hoje você roda `assistente atualizar` sobre código que o
  agente escreveu; aqui o clique em Aplicar faz esse papel. Nada é aplicado sozinho.
- **O agente não consegue se aplicar.** Dentro do ai-jail ele não vê o token
  (`~/.config/assistente` não é montado) nem alcança a porta do hubd (a rede só libera as do
  túnel, por `--forward-port`). Validar as duas coisas na F8.5. A única porta do hubd que entra
  no jail é a API dos agentes (`agent_listen`, F9.5): um roteador à parte que só aceita pedidos
  de comando e não tem nenhuma rota `/api/*`, nem as de aplicar ou aprovar.
- **O build roda fora do jail**, com Docker e acesso aos dados de todos os perfis da VM. A
  confiança é a mesma de hoje (o código aprovado roda como hubd). Para reduzir o risco, o
  build sai de um commit num worktree limpo (nada do `target/` do agente) e só depois do
  `just check` verde.
- **A UI remota tem acesso aos comandos de host** (área de transferência, Cursor, token).
  Quem escreve em `~/.assistente/versoes` controla isso, e essa pasta fica fora de qualquer
  `--rw-map`. Manter a casca com poucos comandos continua sendo a defesa.

## Dependências e pontos a validar

- **F3.1 (Docker no jail)** para o agente da aba rodar `./dev just check` antes de commitar.
  Sem ela, o check do atualizador ainda segura, mas o agente trabalha às cegas.
- Navegar a janela principal para `http://` e chamar IPC de lá com `remote.urls` no
  WebKitGTK (Tauri 2). Validar primeiro, numa prova mínima: é a parte que pode não funcionar.
- O devenv precisa rodar dentro da VM (`./dev` com o Docker de lá) com os volumes de cache.
- Restart do hubd com o atualizador rodando: o atualizador sobrevive (`KillMode=process`)?
