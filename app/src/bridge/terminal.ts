// Conexão de um terminal com o hubd (protocolo na spec 02). No app desktop o WebSocket passa
// pelo plugin do Tauri: o WebKit bloqueia ws:// a partir de tauri://localhost (mixed content).
import { isTauri } from "@tauri-apps/api/core";
import TauriWebSocket from "@tauri-apps/plugin-websocket";

export type TerminalEvents = {
  onData: (bytes: Uint8Array) => void;
  /** A sessão terminou (o agente saiu) ou a conexão caiu. */
  onClose: (reason: "exit" | "error", detail?: TerminalDetail) => void;
};

/** Detalhe do fim: o status de saída do processo (F14.3) ou a mensagem do erro. */
export type TerminalDetail = {
  /** Código de saída do processo da sessão (`exit` do hubd). */
  status?: number;
  message?: string;
};

export type TerminalConnection = {
  write: (text: string) => void;
  resize: (cols: number, rows: number) => void;
  close: () => void;
};

const encoder = new TextEncoder();

function handleText(text: string, events: TerminalEvents) {
  try {
    const message = JSON.parse(text) as { type: string; status?: number; message?: string };
    if (message.type === "exit") events.onClose("exit", { status: message.status });
    if (message.type === "error") events.onClose("error", { message: message.message });
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
    else if (message.type === "Close" && !closed)
      events.onClose("error", { message: "conexão fechada" });
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
      if (!closed) events.onClose("error", { message: "conexão fechada" });
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

/** F8.1: espera da tentativa n — 1 s, 2 s, 4 s… até o teto de 30 s. */
export function backoffDelayMs(attempt: number, base = 1000, cap = 30_000): number {
  return Math.min(cap, base * 2 ** (attempt - 1));
}

export type PersistentEvents = {
  onData: (bytes: Uint8Array) => void;
  /** Conexão estabelecida (a primeira ou depois de retries). */
  onOpen: () => void;
  /** A conexão caiu e uma nova tentativa está agendada. */
  onReconnecting: (attempt: number, delayMs: number) => void;
  /** O agente saiu (`exit` do hubd): não reconecta. `status` é o código de saída (F14.3). */
  onExit: (status?: number) => void;
};

type PersistentDeps = {
  /** Injetável para teste; padrão: `connectTerminal`. */
  connect?: typeof connectTerminal;
  sleep?: (ms: number) => Promise<void>;
};

/**
 * Terminal que reconecta sozinho (F8.1): o WS caiu sem `exit` (hubd reiniciou, rede
 * piscou) → nova tentativa com backoff de 1 s até 30 s; o backoff zera a cada conexão
 * aceita. Escrita enquanto desconectado é descartada (a faixa na UI avisa); o último
 * resize é reaplicado ao reconectar.
 */
export function connectPersistent(
  url: string,
  events: PersistentEvents,
  deps: PersistentDeps = {},
): TerminalConnection {
  const connect = deps.connect ?? connectTerminal;
  const sleep =
    deps.sleep ?? ((ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)));

  let stopped = false;
  let inner: TerminalConnection | null = null;
  let pendingResize: [number, number] | null = null;
  let wakeUp: (() => void) | null = null;
  let wakeClosed: (() => void) | null = null;
  const stopSignal = new Promise<void>((resolve) => {
    wakeUp = resolve;
  });

  async function loop() {
    let attempt = 0;
    while (!stopped) {
      if (attempt > 0) {
        const delay = backoffDelayMs(attempt);
        events.onReconnecting(attempt, delay);
        await Promise.race([sleep(delay), stopSignal]);
        if (stopped) return;
      }
      let exitRequested = false;
      let exitStatus: number | undefined;
      const closed = new Promise<void>((resolve) => {
        wakeClosed = resolve;
        connect(url, {
          onData: events.onData,
          onClose: (reason, detail) => {
            if (reason === "exit") {
              exitRequested = true;
              exitStatus = detail?.status;
            }
            resolve();
          },
        })
          .then((connection) => {
            if (stopped) {
              connection.close();
              resolve();
              return;
            }
            inner = connection;
            attempt = 0;
            if (pendingResize) connection.resize(pendingResize[0], pendingResize[1]);
            events.onOpen();
          })
          .catch(() => resolve());
      });
      await closed;
      wakeClosed = null;
      inner = null;
      if (stopped) return;
      if (exitRequested) {
        events.onExit(exitStatus);
        return;
      }
      attempt += 1;
    }
  }

  void loop().catch((error) => console.error("terminal persistente:", error));

  return {
    write: (data) => inner?.write(data),
    resize: (cols, rows) => {
      pendingResize = [cols, rows];
      inner?.resize(cols, rows);
    },
    close: () => {
      stopped = true;
      wakeUp?.();
      wakeClosed?.();
      inner?.close();
    },
  };
}
