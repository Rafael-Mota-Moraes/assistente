# 03 — Interface

Cobre R1. Funcionalidades F0.4, F1.5, F2.4–F2.6, F4.3, F5.5, F5.6, F6.6, F12.1, F12.2, F12.4.

## Tecnologia

Tauri 2 + React 19 + Vite + Tailwind 4 + zustand + lucide-react; terminais com xterm.js (F2.4).
A UI fala **direto** com o hubd por HTTP/WS (`src/bridge/hub.ts`). O lado Rust do Tauri só faz
o que exige o host (`src/bridge/host.ts` ↔ `app/src-tauri/src/main.rs`):

| Comando | Faz |
|---|---|
| `hub_connection` | URL do hubd (`http://<vm.ip>:<porta>` ou `ASSISTENTE_HUB_URL`) + token lido do host |
| `vm_status` | `incus list` → `running|stopped|missing|no-incus|unknown` |
| `vm_start` / `vm_stop` | `incus start|stop <vm.name>` (F1.5; botões na barra de status) |
| `/api/memory` | Saúde do ai-memory **local** na VM (`GET /healthz` em `[memory].port`, F5.0) |
| `open_in_cursor` | abre `cursor <caminho>` se o caminho estiver dentro de uma raiz de perfil |
| `open_url` | abre `xdg-open <url>` no navegador padrão; só `http://`/`https://` (links do terminal, F2.4) |
| `open_memory_panel` / `update_memory_panel_bounds` / `close_memory_panel` | painel da memória (F5.5): webview filha `memoria` com a web UI do ai-memory via `/memoria/entrar` do hubd, ancorada sobre a área de conteúdo principal (bounds em px de CSS vêm da UI; no Linux o wry (0.57) ignora os bounds do `add_child` — as duas webviews dividem a GtkBox da janela — então a webview principal mora num `GtkOverlay` com palco `GtkFixed` e a filha é ancorada por coordenadas de CSS via `with_webview`; fora do Linux o Rust converte px lógicos em **físicos**, pois o wry no X11 reescala lógicos pelo DPI da tela, tauri#15656); fora das capabilities, sem acesso aos comandos |
| `open_preview_panel` / `update_preview_panel_bounds` / `close_preview_panel` | preview do projeto (F12.1): webview filha `preview` com a página do dev server (`preview_url` das configurações do projeto, F9.2), ancorada como o painel da memória; chamar `open_preview_panel` de novo recarrega a página |
| `preview_panel_visible` | mostra/esconde a webview do preview: um modal do app (a engrenagem, F9.3) precisa ficar por cima dela, que é um widget nativo acima da UI |
| `preview_picker` | liga/desliga o modo "apontar" na página do preview por `eval` (a API `window.__assistentePick.setEnabled` do `pick.js`, F12.2) |

**Mixed content (validado em 2026-10-19):** no Linux a página do app vem de `tauri://localhost`,
que o WebKit trata como origem segura, e `fetch` para `http://10.77.0.10` é bloqueado em
silêncio. Por isso o HTTP passa pelo `tauri-plugin-http` (`hubFetch` em `bridge/host.ts`),
com escopo em `app/src-tauri/capabilities/default.json` (mudou IP/porta da VM? atualize lá).
O WebSocket dos terminais (F2.3) vai ter o mesmo problema: usar `tauri-plugin-websocket`.

Fora do Tauri (`pnpm dev` no navegador), `host.ts` devolve valores neutros e o hub é
`http://127.0.0.1:7878`, o que basta para desenvolver a UI.

## Apontar (F12.2)

O preview do projeto (F12.1) abre numa webview filha, fora das capabilities: a página
apontada não alcança os comandos do app. O modo apontar é um script injetado nessa
webview (`js_init_script` do plugin `pick`, fora do bundle do projeto, em
`app/src-tauri/src/pick.js`):

- botão flutuante **Apontar** (canto superior direito) liga o modo, e o botão de apontar da
  barra do preview faz o mesmo pelo `preview_picker` (`window.__assistentePick.setEnabled`);
- ligado, o elemento sob o mouse ganha contorno e o clique é tomado na **fase de
  captura**: a página não recebe o clique, a ação do botão não dispara;
- `Esc` cancela; um novo clique troca a seleção.

O script entra em toda webview e se decide pelo rótulo (`__TAURI_INTERNALS__`): a
janela principal (`main`) e o painel da memória (`memoria`) ficam de fora, e sem
rótulo o botão não aparece (a dúvida vale mais que o botão sobrando).

O pick (tag, id, classes, `role`/`aria`, texto curto, trecho de HTML, seletor CSS,
caminho no DOM, retângulo, viewport/scroll e URL) sai da página por uma navegação
`assistente-pick:<payload>`: sem IPC e sem CORS na página remota. O Rust intercepta a
navegação (`on_navigation` do plugin `pick`, em `app/src-tauri/src/pick.rs`), descarta
payload acima de 64 KiB, troca a URL do payload pela **URL real da webview** (a da
página não é confiável), emite o evento para a janela principal e cancela a navegação.
O esquema não precisa ser registrado no app: o WebKit entrega a navegação para a
política e ela é cancelada antes de a página trocar.

| Evento | Payload |
|---|---|
| `preview-pick` | `PickedElement` — o elemento apontado (espelha `app/src/bridge/types.ts`) |

O `PreviewPanel` escuta o evento e abre a caixa de pedido do F12.4
(`features/pick/PickDialog.tsx`) para o projeto do preview; o `PickedElement` vira o
`PickDetail` que o hubd recebe em `features/pick/detail.ts`. Enquanto a caixa está aberta,
a webview do preview fica escondida (`preview_panel_visible`), senão ela ficaria por cima da
caixa; e o Esc da caixa (ou de um campo) não fecha o preview.

Para conferir na mão: sirva uma página de teste com um botão que muda de cor
(`python3 -m http.server`), abra no preview do projeto (F12.1), ligue o **Apontar** e
clique no botão: a cor não muda (o clique foi capturado) e a caixa de pedido abre com
o botão escolhido.

## Layout

```
┌──────────────┬───────────────────────────────────────────────────────┐
│ ● Assistente │ game              [Claude] [OpenCode] [Terminal]      │
│ [Pessoal|Trab]├──────────────────────────────────────────────────────┤
│              │ claude-1 │ opencode-1 │ +                              │
│ ▸ game    🌙 │ ┌──────────────────────────────────────────────────┐  │
│   estudos    │ │ terminal xterm.js (sessão tmux na VM)            │  │
│   assistente │ │                                                  │  │
│              │ └──────────────────────────────────────────────────┘  │
│ [🌙 Hora de  │                                                       │
│    dormir ]  │                                                       │
├──────────────┴───────────────────────────────────────────────────────┤
│ ● VM ligada  ● hubd 0.1.0  ● 9router  ● ai-memory     notebook: nb   │
└──────────────────────────────────────────────────────────────────────┘
```

- **Tema**: escuro, fundo com dois brilhos radiais (violeta/azul), painéis translúcidos.
  A cor do perfil ativo tinge o seletor (e, na F2.4, a borda da aba ativa), para nunca
  confundir pessoal com trabalho.
- **Terminal** (F9.1, âmbar): abre uma aba com `bash` na pasta do projeto, na VM e **fora do
  jail**, com o env do perfil. Para o que o agente não consegue rodar (Docker, rede, `git push`)
  sem procurar a pasta num terminal do PC. A aba também fica âmbar.
- 🌙 ao lado do projeto = tem `.assistente.toml` (pronto para o modo dormir).
- Passar o mouse num projeto mostra a **engrenagem** (F9.3) e "abrir no Cursor". A engrenagem
  abre as configurações do jail do projeto: Docker, rede completa (aviso vermelho), hosts
  liberados, comandos sempre permitidos e a **URL do preview** (F12.1). Valem para as sessões
  novas; o Docker ligado pelo `[jail]` do `.assistente.toml` aparece marcado e travado.
- **Preview** (F12.1): botão "Visualizar" junto dos agentes abre a página do dev server do
  projeto numa aba (a sessão continua viva). O chrome (URL, recarregar, apontar, fechar) é
  React; a webview filha preenche o retângulo e acompanha o redimensionamento da janela.
  A URL se edita na engrenagem, com a dica de que o endereço é visto do PC: projeto rodando
  na VM usa o IP dela.

## Apontar e pedir (F12.4)

O modal do apontamento (`app/src/features/pick/PickDialog.tsx`) mostra o elemento apontado (URL,
seletor, texto e onde ele está), deixa editar o recado e escolher para onde o pedido vai:

| Alvo | O que o hubd faz |
|---|---|
| **Sessão** | Confere que a sessão é de agente (a aba Terminal nunca recebe), do projeto e está viva (painel morto da F14.3 = 409); cola o prompt no prompt do agente com `set-buffer` + `paste-buffer`, **sem Enter** (o Enter é do usuário: não aprova pergunta pendente nem executa texto no shell). A UI leva para a aba e avisa "confirme no terminal" |
| **Quadro** | Vira card no backlog (F11.1), com o texto inteiro: o clone do notebook não tem o arquivo do pick |

Rotas (o token do hub, como as outras):

- `POST /api/projects/{perfil}/{projeto}/picks` — envia o pedido. Devolve o prompt usado, a
  sessão (ou o card) e o arquivo do pick.
- `POST /api/projects/{perfil}/{projeto}/picks/preview` — só o prompt que será enviado, para o
  modal mostrar antes de mandar (o prompt é do hubd: URL, onde, seletor, texto e pedido, em uma
  linha só).

O detalhe do pick fica em `.assistente/picks/<data-hora>.md`, dentro do projeto e fora do git
(`info/exclude`, como o `.ai-jail` da F0.5).

## Estados

| Situação | O que a UI mostra |
|---|---|
| Sem config no host | Tela "rode ./instalar.sh" com o erro do `hub_connection` |
| VM parada / não criada | Ponto amarelo/vermelho + botão ligar (F1.5) |
| hubd fora do ar | Área central "Sem conexão com o hubd" com o erro e como resolver |
| Notebook/túnel caído | 9router vermelho; memória local verde após F5.0 (hoje ai-memory pode vermelho se ainda via túnel) |
| Memória (F5.6) | Clique no ponto **ai-memory** abre painel: paths (`data_dir` / `mirror_dir`), erro do probe, último commit do wiki, sync (F5.4), link para web UI |
| Claude sem login no perfil (F2.7) | Faixa âmbar com `bin/assistente login claude <perfil>`; o botão Claude não abre a aba (que morreria na hora) e repete o aviso. OpenCode e Terminal seguem normais |
| Sessão que morreu (F14.3) | O terminal fica com o que o processo imprimiu e uma faixa "Sessão encerrada (status N)"; a aba continua aberta até você encerrar a sessão |
| Pedido fora do jail (F9.7) | Cartão âmbar no canto: comando inteiro, projeto e aba, pasta; Aprovar / Negar / "Aprovar e sempre permitir neste projeto" (confirma com aviso). Bolinha âmbar na aba e contador no projeto. Recentes recolhidos com o resultado. Poll de 2 s |
| Modo dormir ativo | Botão vira "Dormindo… (tarefa N, log)" com parar; de manhã, modal "Bom dia" (F6.6, botão na lateral) |

## Atalhos (F2.4)

`Ctrl+Shift+T` nova sessão (último agente), `Ctrl+Shift+W` fecha a aba (não mata a sessão),
`Ctrl+Tab` próxima aba, `Ctrl+1..9` aba N, `Ctrl+Shift+P` troca de perfil.

## Polling

`usePolling` a cada 5 s: health, VM, notebook e perfis. Na F2 entram eventos por WS
(`/api/events`) para não depender só de polling.
