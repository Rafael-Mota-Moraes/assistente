import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as host from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import { useAppStore } from "../../store";
import { MemoryButton } from "./MemoryButton";

const memory = { port: 49374, mirror_dir: "/m", up: true, last_error: null, checked_at: 0 };

function fakeHub() {
  return {
    memoryLoginUrl: () => "http://10.77.0.10:7878/memoria/entrar?token=t",
  } as unknown as HubClient;
}

describe("MemoryButton", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("abre a janela da memória pela entrada do hubd", async () => {
    const open = vi.spyOn(host, "openMemoryWindow").mockResolvedValue();
    useAppStore.setState({ hub: fakeHub(), memory });
    render(<MemoryButton />);

    fireEvent.click(screen.getByRole("button", { name: /Memória/ }));
    await waitFor(() =>
      expect(open).toHaveBeenCalledWith("http://10.77.0.10:7878/memoria/entrar?token=t"),
    );
  });

  it("fica desligado com o ai-memory fora do ar", () => {
    useAppStore.setState({ hub: fakeHub(), memory: { ...memory, up: false } });
    render(<MemoryButton />);
    expect(screen.getByRole("button", { name: /Memória/ })).toBeDisabled();
  });

  it("mostra o erro no toast", async () => {
    vi.spyOn(host, "openMemoryWindow").mockRejectedValue(new Error("sem janela"));
    useAppStore.setState({ hub: fakeHub(), memory });
    render(<MemoryButton />);

    fireEvent.click(screen.getByRole("button", { name: /Memória/ }));
    await waitFor(() => expect(useAppStore.getState().toast).toContain("sem janela"));
  });
});
