import { act, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Health } from "../../bridge/types";
import { useAppStore } from "../../store";
import { reloadPage } from "./uiUpdate";
import { usePolling } from "./usePolling";

vi.mock("../../bridge/host", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../bridge/host")>();
  return { ...actual, vmStatus: vi.fn(() => Promise.resolve(null)) };
});

vi.mock("./uiUpdate", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./uiUpdate")>();
  return { ...actual, reloadPage: vi.fn() };
});

function health(ui: string | null): Health {
  return {
    ok: true,
    version: "0.1.0",
    uptime_seconds: 1,
    commit: { hubd: "abc1234", ui },
    self_update: null,
  };
}

/** Hub falso: só o que o `usePolling` chama. */
function fakeHub(overrides: Record<string, unknown> = {}) {
  return {
    health: vi.fn(),
    notebook: vi.fn(() => Promise.resolve(null)),
    memory: vi.fn(() => Promise.resolve(null)),
    profiles: vi.fn(() => Promise.resolve([])),
    sessions: vi.fn(() => Promise.resolve([])),
    updateStatus: vi.fn(() => Promise.resolve({ running: false, shell_changed: false })),
    ...overrides,
  };
}

function Probe() {
  usePolling();
  return null;
}

/** Deixa o tick inicial (promessas já resolvidas) terminar. */
async function assentar() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  sessionStorage.clear();
  useAppStore.setState({ hub: null, health: null, toast: null });
});

afterEach(() => {
  vi.useRealTimers();
  vi.mocked(reloadPage).mockClear();
  useAppStore.setState({ hub: null, health: null, toast: null });
});

describe("usePolling", () => {
  it("não recarrega enquanto a UI em uso não muda", async () => {
    const hub = fakeHub({ health: vi.fn(() => Promise.resolve(health("aaaa111"))) });
    useAppStore.setState({ hub: hub as never });
    render(<Probe />);
    await assentar();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(12_000);
    });
    expect(reloadPage).not.toHaveBeenCalled();
  });

  it("recarrega quando o commit da UI muda e guarda o aviso", async () => {
    const hub = fakeHub({
      health: vi.fn().mockResolvedValueOnce(health("aaaa111")).mockResolvedValue(health("bbbb222")),
      updateStatus: vi.fn(() => Promise.resolve({ running: false, shell_changed: true })),
    });
    useAppStore.setState({ hub: hub as never });
    render(<Probe />);
    await assentar();

    // Próximo ciclo do poll (5 s) nota o commit novo e agenda o reload.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(reloadPage).not.toHaveBeenCalled();
    expect(useAppStore.getState().toast).toContain("Versão nova aplicada");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_200);
    });
    expect(reloadPage).toHaveBeenCalledTimes(1);
    expect(JSON.parse(sessionStorage.getItem("assistente.ui-update") ?? "null")).toEqual({
      commit: "bbbb222",
      shell: true,
    });
  });

  it("espera o atualizador terminar antes de recarregar", async () => {
    const hub = fakeHub({
      health: vi.fn().mockResolvedValueOnce(health("aaaa111")).mockResolvedValue(health("bbbb222")),
      updateStatus: vi
        .fn()
        .mockResolvedValueOnce({ running: true, shell_changed: false })
        .mockResolvedValue({ running: false, shell_changed: false }),
    });
    useAppStore.setState({ hub: hub as never });
    render(<Probe />);
    await assentar();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    // Ainda rodando: sem reload; confere de novo em 1 s.
    expect(reloadPage).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_200);
    });
    expect(reloadPage).toHaveBeenCalledTimes(1);
    expect(hub.updateStatus).toHaveBeenCalledTimes(2);
  });

  it("não recarrega se o atualizador voltou para a versão anterior", async () => {
    const hub = fakeHub({
      health: vi
        .fn()
        .mockResolvedValueOnce(health("aaaa111"))
        .mockResolvedValueOnce(health("bbbb222"))
        .mockResolvedValue(health("aaaa111")),
      updateStatus: vi.fn(() => Promise.resolve({ running: true, shell_changed: false })),
    });
    useAppStore.setState({ hub: hub as never });
    render(<Probe />);
    await assentar();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    // Voltou: o commit em uso é o antigo de novo; nenhum reload é agendado.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4_000);
    });
    expect(reloadPage).not.toHaveBeenCalled();
    expect(sessionStorage.getItem("assistente.ui-update")).toBeNull();
  });
});
