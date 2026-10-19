# 03 — Interface

Cobre R1. Funcionalidades F0.4, F1.5, F2.4–F2.6, F4.3, F5.5, F6.6.

## Tecnologia

Tauri 2 + React 19 + Vite + Tailwind 4 + zustand + lucide-react; terminais com xterm.js (F2.4).
A UI fala **direto** com o hubd por HTTP/WS (`src/bridge/hub.ts`). O lado Rust do Tauri só faz
o que exige o host (`src/bridge/host.ts` ↔ `app/src-tauri/src/main.rs`):

| Comando | Faz |
|---|---|
| `hub_connection` | URL do hubd (`http://<vm.ip>:<porta>` ou `ASSISTENTE_HUB_URL`) + token lido do host |
| `vm_status` | `incus list` → `running|stopped|missing|no-incus|unknown` |
| `open_in_cursor` | abre `cursor <caminho>` se o caminho estiver dentro de uma raiz de perfil |

**Mixed content (validado em 2026-10-19):** no Linux a página do app vem de `tauri://localhost`,
que o WebKit trata como origem segura, e `fetch` para `http://10.77.0.10` é bloqueado em
silêncio. Por isso o HTTP passa pelo `tauri-plugin-http` (`hubFetch` em `bridge/host.ts`),
com escopo em `app/src-tauri/capabilities/default.json` (mudou IP/porta da VM? atualize lá).
O WebSocket dos terminais (F2.3) vai ter o mesmo problema: usar `tauri-plugin-websocket`.

Fora do Tauri (`pnpm dev` no navegador), `host.ts` devolve valores neutros e o hub é
`http://127.0.0.1:7878`, o que basta para desenvolver a UI.

## Layout

```
┌──────────────┬───────────────────────────────────────────────────────┐
│ ● Assistente │ game                         [Claude] [OpenCode]      │
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
- 🌙 ao lado do projeto = tem `.assistente.toml` (pronto para o modo dormir).
- Passar o mouse num projeto mostra "abrir no Cursor".

## Estados

| Situação | O que a UI mostra |
|---|---|
| Sem config no host | Tela "rode ./instalar.sh" com o erro do `hub_connection` |
| VM parada / não criada | Ponto amarelo/vermelho + botão ligar (F1.5) |
| hubd fora do ar | Área central "Sem conexão com o hubd" com o erro e como resolver |
| Notebook/túnel caído | Pontos vermelhos de 9router/ai-memory; sessões continuam (Claude direto funciona) |
| Modo dormir ativo | Botão vira "Dormindo… (tarefa N, log)" com parar; de manhã, modal "Bom dia" (F6.6) |

## Atalhos (F2.4)

`Ctrl+Shift+T` nova sessão (último agente), `Ctrl+Shift+W` fecha a aba (não mata a sessão),
`Ctrl+Tab` próxima aba, `Ctrl+1..9` aba N, `Ctrl+Shift+P` troca de perfil.

## Polling

`usePolling` a cada 5 s: health, VM, notebook e perfis. Na F2 entram eventos por WS
(`/api/events`) para não depender só de polling.
