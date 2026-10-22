import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HubClient } from "../../bridge/hub";
import { useAppStore } from "../../store";
import { TerminalView } from "./TerminalView";

/** O xterm.js não roda no jsdom: um terminal falso, só o essencial. */
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    textarea = null;
    parser = { registerOscHandler: () => ({ dispose: () => {} }) };
    loadAddon() {}
    open() {}
    focus() {}
    write() {}
    dispose() {}
    attachCustomKeyEventHandler() {}
    onData() {
      return { dispose: () => {} };
    }
    onResize() {
      return { dispose: () => {} };
    }
    hasSelection() {
      return false;
    }
    getSelection() {
      return "";
    }
    paste() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));
vi.mock("@xterm/addon-web-links", () => ({ WebLinksAddon: class {} }));
vi.mock("../../bridge/host", () => ({ openUrl: vi.fn() }));

/** O terminal falso guarda os eventos para o teste simular a morte da sessão. */
const fake = vi.hoisted(() => ({
  events: null as null | {
    onData: (bytes: Uint8Array) => void;
    onOpen: () => void;
    onReconnecting: (attempt: number, delayMs: number) => void;
    onExit: (status?: number) => void;
  },
}));
vi.mock("../../bridge/terminal", () => ({
  connectPersistent: (_url: string, events: NonNullable<typeof fake.events>) => {
    fake.events = events;
    return { write: vi.fn(), resize: vi.fn(), close: vi.fn() };
  },
}));

describe("TerminalView", () => {
  beforeEach(() => {
    // O jsdom não tem ResizeObserver.
    globalThis.ResizeObserver = class {
      observe() {}
      unobserve() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver;
    fake.events = null;
    useAppStore.setState({
      hub: { terminalUrl: (id: string) => `ws://hub/${id}` } as unknown as HubClient,
    });
  });

  it("a sessão que morre mostra o status de saída (F14.3)", async () => {
    render(<TerminalView sessionId="pessoal--x--claude-1" />);
    await waitFor(() => expect(fake.events).not.toBeNull());
    act(() => fake.events?.onExit(127));
    expect(screen.getByText("Sessão encerrada (status 127).")).toBeInTheDocument();
  });

  it("sem status (sessão encerrada na mão) avisa que acabou", async () => {
    render(<TerminalView sessionId="pessoal--x--claude-1" />);
    await waitFor(() => expect(fake.events).not.toBeNull());
    act(() => fake.events?.onExit());
    expect(screen.getByText("Sessão encerrada.")).toBeInTheDocument();
  });

  it("encerrar a sessão depois não apaga o status da morte (F14.3)", async () => {
    render(<TerminalView sessionId="pessoal--x--claude-1" />);
    await waitFor(() => expect(fake.events).not.toBeNull());
    act(() => fake.events?.onExit(127));
    act(() => fake.events?.onExit());
    expect(screen.getByText("Sessão encerrada (status 127).")).toBeInTheDocument();
  });
});
