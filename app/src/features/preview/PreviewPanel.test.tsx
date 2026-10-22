import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HubClient } from "../../bridge/hub";
import { useAppStore } from "../../store";
import { PreviewPanel } from "./PreviewPanel";

function fakeHub(previewUrl: string | null) {
  return {
    projectSettings: vi.fn().mockResolvedValue({
      settings: {
        docker: false,
        allow_hosts: [],
        full_network: false,
        allowed_commands: [],
        preview_url: previewUrl,
      },
      docker_from_project_file: false,
    }),
  } as unknown as HubClient;
}

describe("PreviewPanel", () => {
  it("mostra a URL do preview e a página num iframe fora do app desktop", async () => {
    useAppStore.setState({
      hub: fakeHub("http://10.77.0.10:5173"),
      activeProfile: "pessoal",
      activeProject: "game",
    });
    const { container } = render(<PreviewPanel />);

    expect(await screen.findByText("http://10.77.0.10:5173")).toBeInTheDocument();
    const frame = container.querySelector("iframe");
    expect(frame).toHaveAttribute("src", "http://10.77.0.10:5173");
  });

  it("sem URL avisa e dá o atalho para a engrenagem do projeto", async () => {
    useAppStore.setState({
      hub: fakeHub(null),
      activeProfile: "pessoal",
      activeProject: "game",
    });
    render(<PreviewPanel />);

    expect(await screen.findByText(/não tem URL de preview/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /configurações do projeto/i }));
    expect(useAppStore.getState().projectSettings).toEqual({
      profile: "pessoal",
      project: "game",
    });
  });

  it("o X da barra fecha a aba do preview", async () => {
    useAppStore.setState({
      hub: fakeHub("http://10.77.0.10:5173"),
      activeProfile: "pessoal",
      activeProject: "game",
      views: [{ key: "pessoal/game", kind: "preview" }],
      activeView: { key: "pessoal/game", kind: "preview" },
    });
    render(<PreviewPanel />);

    fireEvent.click(screen.getByTitle("Fechar (Esc)"));
    expect(useAppStore.getState().views).toHaveLength(0);
  });
});
