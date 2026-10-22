import { describe, expect, it, vi } from "vitest";
import { backoffDelayMs, connectPersistent, type TerminalEvents } from "./terminal";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function fakeConnection() {
  return {
    write: vi.fn<(data: string) => void>(),
    resize: vi.fn<(cols: number, rows: number) => void>(),
    close: vi.fn<() => void>(),
  };
}

/** Conector falso: guarda os events de cada chamada para o teste simular quedas. */
function fakeConnector(failFirst = 0) {
  const seen: TerminalEvents[] = [];
  const connections: ReturnType<typeof fakeConnection>[] = [];
  let calls = 0;
  const connect = vi.fn(async (_url: string, events: TerminalEvents) => {
    calls += 1;
    if (calls <= failFirst) throw new Error("hubd fora do ar");
    seen.push(events);
    const connection = fakeConnection();
    connections.push(connection);
    return connection;
  });
  return { connect, seen, connections };
}

/** Sleep controlado pelo teste: cada retry fica aguardando `release`. */
function gatedSleep() {
  const delays: number[] = [];
  let release: (() => void) | null = null;
  const sleep = (ms: number) => {
    delays.push(ms);
    return new Promise<void>((resolve) => {
      release = resolve;
    });
  };
  /** Deixa o loop passar pelo backoff atual e chegar à próxima conexão. */
  const next = async () => {
    await flush();
    release?.();
    release = null;
    await flush();
  };
  return { sleep, delays, next };
}

function persistentEvents() {
  return {
    onData: vi.fn(),
    onOpen: vi.fn(),
    onReconnecting: vi.fn(),
    onExit: vi.fn(),
  };
}

describe("backoffDelayMs", () => {
  it("dobra de 1 s em 1 s até o teto de 30 s", () => {
    expect(backoffDelayMs(1)).toBe(1000);
    expect(backoffDelayMs(2)).toBe(2000);
    expect(backoffDelayMs(3)).toBe(4000);
    expect(backoffDelayMs(5)).toBe(16000);
    expect(backoffDelayMs(6)).toBe(30000);
    expect(backoffDelayMs(99)).toBe(30000);
  });
});

describe("connectPersistent", () => {
  it("conecta e repete o resize pedido antes da conexão abrir", async () => {
    const { connect, connections } = fakeConnector();
    const events = persistentEvents();
    const link = connectPersistent("ws://x", events, { connect, sleep: async () => {} });
    link.resize(120, 40);
    await flush();
    expect(events.onOpen).toHaveBeenCalledTimes(1);
    expect(connections[0].resize).toHaveBeenCalledWith(120, 40);
    link.close();
  });

  it("WS caiu sem exit → reconecta com backoff e emite onOpen de novo", async () => {
    const { connect, seen } = fakeConnector();
    const gate = gatedSleep();
    const events = persistentEvents();
    const link = connectPersistent("ws://x", events, { connect, sleep: gate.sleep });
    await flush();
    expect(connect).toHaveBeenCalledTimes(1);

    seen[0].onClose("error", { message: "hubd reiniciou" });
    await gate.next();

    expect(events.onReconnecting).toHaveBeenCalledWith(1, 1000);
    expect(connect).toHaveBeenCalledTimes(2);
    expect(events.onOpen).toHaveBeenCalledTimes(2);
    expect(events.onExit).not.toHaveBeenCalled();
    link.close();
  });

  it("hubd fora do ar: tentativas escalam 1 s, 2 s, 4 s… até conectar", async () => {
    const { connect } = fakeConnector(3); // as 3 primeiras conexões falham
    const gate = gatedSleep();
    const events = persistentEvents();
    const link = connectPersistent("ws://x", events, { connect, sleep: gate.sleep });

    await flush(); // tentativa inicial falha; loop entra no backoff 1
    await gate.next(); // falha → backoff 2
    await gate.next(); // falha → backoff 3
    await gate.next(); // conecta

    expect(gate.delays).toEqual([1000, 2000, 4000]);
    expect(events.onReconnecting).toHaveBeenCalledTimes(3);
    expect(events.onOpen).toHaveBeenCalledTimes(1);
    link.close();
  });

  it("backoff zera depois de reconectar: nova queda volta em 1 s", async () => {
    const { connect, seen } = fakeConnector();
    const gate = gatedSleep();
    const events = persistentEvents();
    const link = connectPersistent("ws://x", events, { connect, sleep: gate.sleep });
    await flush();

    seen[0].onClose("error");
    await gate.next();
    seen[1].onClose("error");
    await gate.next();

    expect(gate.delays).toEqual([1000, 1000]);
    link.close();
  });

  it("exit do agente não reconecta", async () => {
    const { connect, seen } = fakeConnector();
    const events = persistentEvents();
    const link = connectPersistent("ws://x", events, { connect, sleep: async () => {} });
    await flush();

    seen[0].onClose("exit");
    await flush();
    await flush();

    expect(events.onExit).toHaveBeenCalledTimes(1);
    expect(connect).toHaveBeenCalledTimes(1);
    link.close();
  });

  it("exit com status de saída chega no onExit (F14.3)", async () => {
    const { connect, seen } = fakeConnector();
    const events = persistentEvents();
    const link = connectPersistent("ws://x", events, { connect, sleep: async () => {} });
    await flush();

    seen[0].onClose("exit", { status: 127 });
    await flush();
    await flush();

    expect(events.onExit).toHaveBeenCalledWith(127);
    link.close();
  });

  it("close() para o loop mesmo no meio do backoff", async () => {
    const { connect } = fakeConnector(Number.MAX_SAFE_INTEGER); // nunca conecta
    const gate = gatedSleep();
    const events = persistentEvents();
    const link = connectPersistent("ws://x", events, { connect, sleep: gate.sleep });
    await flush(); // primeira falha; parado no backoff

    const callsAtClose = connect.mock.calls.length;
    expect(callsAtClose).toBe(1);
    link.close();
    gate.delays.length = 0;
    await flush();
    await flush();

    expect(connect).toHaveBeenCalledTimes(callsAtClose); // nenhuma tentativa depois do close
    expect(gate.delays).toEqual([]);
    expect(events.onExit).not.toHaveBeenCalled();
  });

  it("dados recebidos chegam ao onData", async () => {
    const { connect, seen } = fakeConnector();
    const events = persistentEvents();
    const link = connectPersistent("ws://x", events, { connect, sleep: async () => {} });
    await flush();
    seen[0].onData(new Uint8Array([1, 2, 3]));
    expect(events.onData).toHaveBeenCalledWith(new Uint8Array([1, 2, 3]));
    link.close();
  });
});
