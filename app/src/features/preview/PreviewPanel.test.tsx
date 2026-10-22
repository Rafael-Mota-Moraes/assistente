import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { previewPanelVisible } from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import type { PickedElement } from "../../bridge/types";
import { useAppStore } from "../../store";
import { PreviewPanel } from "./PreviewPanel";

// O Rust emite o pick (`preview-pick`); aqui o teste faz o papel dele.
const { ouvintes } = vi.hoisted(() => ({
  ouvintes: [] as ((pick: PickedElement) => void)[],
}));

vi.mock("../../bridge/host", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../bridge/host")>()),
  previewPanelVisible: vi.fn().mockResolvedValue(undefined),
  listenPreviewPick: vi.fn(async (handler: (pick: PickedElement) => void) => {
    ouvintes.push(handler);
    return () => {};
  }),
}));

const pick: PickedElement = {
  tag: "button",
  id: "salvar",
  classes: ["btn"],
  role: null,
  aria: { label: "Salvar" },
  text: "Salvar",
  html: '<button id="salvar">Salvar</button>',
  selector: "#salvar",
  dom_path: "body > main > button#salvar",
  rect: { x: 10, y: 20, width: 120, height: 32 },
  viewport: { width: 1280, height: 720 },
  scroll: { x: 0, y: 0 },
  url: "http://10.77.0.10:5173/",
};

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

  it("Esc fecha a aba do preview", async () => {
    useAppStore.setState({
      hub: fakeHub("http://10.77.0.10:5173"),
      activeProfile: "pessoal",
      activeProject: "game",
      views: [{ key: "pessoal/game", kind: "preview" }],
      activeView: { key: "pessoal/game", kind: "preview" },
    });
    render(<PreviewPanel />);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(useAppStore.getState().views).toHaveLength(0);
  });

  it("o pick da página abre a caixa de pedido do projeto e esconde a webview (F12.2/F12.4)", async () => {
    useAppStore.setState({
      hub: fakeHub("http://10.77.0.10:5173"),
      activeProfile: "pessoal",
      activeProject: "game",
      pick: null,
    });
    render(<PreviewPanel />);
    await screen.findByText("http://10.77.0.10:5173");
    await waitFor(() => expect(ouvintes.length).toBeGreaterThan(0));

    act(() => ouvintes[ouvintes.length - 1](pick));

    const aberto = useAppStore.getState().pick;
    expect(aberto?.profile).toBe("pessoal");
    expect(aberto?.project).toBe("game");
    expect(aberto?.pick).toMatchObject({ selector: "#salvar", aria_label: "Salvar" });
    await waitFor(() => expect(previewPanelVisible).toHaveBeenCalledWith(false));
  });

  it("o Esc da caixa do apontar não fecha o preview", async () => {
    useAppStore.setState({
      hub: fakeHub("http://10.77.0.10:5173"),
      activeProfile: "pessoal",
      activeProject: "game",
      views: [{ key: "pessoal/game", kind: "preview" }],
      activeView: { key: "pessoal/game", kind: "preview" },
      pick: {
        profile: "pessoal",
        project: "game",
        pick: { url: "", selector: "#x", text: "", html: "", tag: "button" },
      },
    });
    render(<PreviewPanel />);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(useAppStore.getState().views).toHaveLength(1);

    // Já sem a caixa, o Esc num campo também é do campo.
    useAppStore.setState({ pick: null });
    const campo = document.createElement("textarea");
    document.body.appendChild(campo);
    fireEvent.keyDown(campo, { key: "Escape" });
    expect(useAppStore.getState().views).toHaveLength(1);
    campo.remove();
  });
});
