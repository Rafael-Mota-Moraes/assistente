import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { HubClient } from "../../bridge/hub";
import { useAppStore } from "../../store";
import { MemoryPanel } from "./MemoryPanel";

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

describe("MemoryPanel", () => {
  afterEach(() => {
    useAppStore.setState({ memoryPanelOpen: false, morning: null, projectSettings: null });
  });

  it("fecha junto quando um modal do app abre", async () => {
    useAppStore.setState({ hub: fakeHub(), memory, memoryPanelOpen: true });
    render(<MemoryPanel />);

    useAppStore.setState({ projectSettings: { profile: "pessoal", project: "game" } });
    await waitFor(() => expect(useAppStore.getState().memoryPanelOpen).toBe(false));
  });

  it("mostra a web UI num iframe apontando para a entrada do hubd", () => {
    useAppStore.setState({ hub: fakeHub(), memory, memoryPanelOpen: true });
    const { container } = render(<MemoryPanel />);

    const frame = container.querySelector("iframe");
    expect(frame).not.toBeNull();
    expect(frame).toHaveAttribute("src", "http://10.77.0.10:7878/memoria/entrar?token=t");
    expect(screen.getByText(/wiki do ai-memory na VM/)).toBeInTheDocument();
  });

  it("avisa quando o hubd está fora", () => {
    useAppStore.setState({ hub: null, memory, memoryPanelOpen: true });
    render(<MemoryPanel />);
    expect(screen.getByText("hubd desconectado")).toBeInTheDocument();
  });

  it("fecha pelo botão da barra", async () => {
    useAppStore.setState({ hub: fakeHub(), memory, memoryPanelOpen: true });
    render(<MemoryPanel />);

    fireEvent.click(screen.getByRole("button", { name: "Fechar (Esc)" }));
    await waitFor(() => expect(useAppStore.getState().memoryPanelOpen).toBe(false));
    expect(useAppStore.getState().memoryPanelOpen).toBe(false);
  });

  it("fecha com Esc", async () => {
    useAppStore.setState({ hub: fakeHub(), memory, memoryPanelOpen: true });
    render(<MemoryPanel />);

    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(useAppStore.getState().memoryPanelOpen).toBe(false));
  });

  it("não renderiza nada fechado", () => {
    useAppStore.setState({ hub: fakeHub(), memory });
    const { container } = render(<MemoryPanel />);
    expect(container).toBeEmptyDOMElement();
  });
});
