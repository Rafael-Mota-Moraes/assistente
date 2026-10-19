// Conexão de um terminal com o hubd (protocolo na spec 02). No app desktop o WebSocket passa
// pelo plugin do Tauri: o WebKit bloqueia ws:// a partir de tauri://localhost (mixed content).
import { isTauri } from "@tauri-apps/api/core";
import TauriWebSocket from "@tauri-apps/plugin-websocket";

export type TerminalEvents = {
  onData: (bytes: Uint8Array) => void;
  /** A sessão terminou (o agente saiu) ou a conexão caiu. */
  onClose: (reason: "exit" | "error", detail?: string) => void;
};

export type TerminalConnection = {
  write: (text: string) => void;
  resize: (cols: number, rows: number) => void;
  close: () => void;
};

const encoder = new TextEncoder();

function handleText(text: string, events: TerminalEvents) {
  try {
    const message = JSON.parse(text) as { type: string; message?: string };
    if (message.type === "exit") events.onClose("exit");
    if (message.type === "error") events.onClose("error", message.message);
  } catch {
    events.onData(encoder.encode(text));
  }
}

export async function connectTerminal(
  url: string,
  events: TerminalEvents,
): Promise<TerminalConnection> {
  if (isTauri()) return connectTauri(url, events);
  return connectBrowser(url, events);
}

async function connectTauri(url: string, events: TerminalEvents): Promise<TerminalConnection> {
  const ws = await TauriWebSocket.connect(url);
  let closed = false;
  ws.addListener((message) => {
    if (message.type === "Binary") events.onData(new Uint8Array(message.data));
    else if (message.type === "Text") handleText(message.data, events);
    else if (message.type === "Close" && !closed) events.onClose("error", "conexão fechada");
  });
  const send = (payload: object) => {
    if (!closed) ws.send(JSON.stringify(payload)).catch(() => {});
  };
  return {
    write: (data) => send({ type: "input", data }),
    resize: (cols, rows) => send({ type: "resize", cols, rows }),
    close: () => {
      closed = true;
      ws.disconnect().catch(() => {});
    },
  };
}

function connectBrowser(url: string, events: TerminalEvents): Promise<TerminalConnection> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    let closed = false;
    ws.onmessage = (event) => {
      if (typeof event.data === "string") handleText(event.data, events);
      else events.onData(new Uint8Array(event.data as ArrayBuffer));
    };
    ws.onclose = () => {
      if (!closed) events.onClose("error", "conexão fechada");
    };
    ws.onerror = () => reject(new Error(`não consegui conectar em ${url}`));
    ws.onopen = () => {
      const send = (payload: object) => {
        if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(payload));
      };
      resolve({
        write: (data) => send({ type: "input", data }),
        resize: (cols, rows) => send({ type: "resize", cols, rows }),
        close: () => {
          closed = true;
          ws.close();
        },
      });
    };
  });
}
