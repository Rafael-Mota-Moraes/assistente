import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HubClient } from "../../bridge/hub";
import { useAppStore } from "../../store";
import { MemoryButton } from "./MemoryButton";

const memory = {
  port: 49374,
  data_dir: "/d",
  mirror_dir: "/m",
  up: true,
  last_error: null,
  checked_at: 0,
  server_version: null,
  last_commit: null,
  web_url: "/web",
};

function fakeHub() {
  return {
    memoryLoginUrl: () => "http://10.77.0.10:7878/memoria/entrar?token=t",
  } as unknown as HubClient;
}

describe("MemoryButton", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    useAppStore.setState({ memoryPanelOpen: false });
  });

  it("abre o painel de memória na janela principal", async () => {
    useAppStore.setState({ hub: fakeHub(), memory });
    render(<MemoryButton />);

    fireEvent.click(screen.getByRole("button", { name: /Memória/ }));
    await waitFor(() => expect(useAppStore.getState().memoryPanelOpen).toBe(true));
  });

  it("clicar de novo fecha o painel", async () => {
    useAppStore.setState({ hub: fakeHub(), memory, memoryPanelOpen: true });
    render(<MemoryButton />);

    fireEvent.click(screen.getByRole("button", { name: /Memória/ }));
    await waitFor(() => expect(useAppStore.getState().memoryPanelOpen).toBe(false));
  });

  it("fica desligado com o ai-memory fora do ar", () => {
    useAppStore.setState({ hub: fakeHub(), memory: { ...memory, up: false } });
    render(<MemoryButton />);
    expect(screen.getByRole("button", { name: /Memória/ })).toBeDisabled();
  });
});
