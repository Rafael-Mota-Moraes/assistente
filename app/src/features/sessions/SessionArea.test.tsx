import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HubClient } from "../../bridge/hub";
import type { SessionInfo } from "../../bridge/types";
import { useAppStore } from "../../store";
import { SessionArea, tabLabel } from "./SessionArea";
import { useShortcuts } from "./shortcuts";

vi.mock("../editor/EditorPanel", () => ({
  EditorPanel: () => <div>editor</div>,
}));

vi.mock("./TerminalView", () => ({
  TerminalView: ({ sessionId }: { sessionId: string }) => <div>terminal {sessionId}</div>,
}));

/** Como o App: atalhos globais + a área de sessões. */
function Harness() {
  useShortcuts();
  return <SessionArea />;
}

const session = (id: string, project: string, agent: SessionInfo["agent"]): SessionInfo => ({
  id,
  profile: "pessoal",
  project,
  agent,
  created_at: 0,
  attached: 0,
});

describe("SessionArea", () => {
  it("Quadro e Editor são abas: abrem, trocam com as sessões e fecham pelo X", async () => {
    useAppStore.setState({
      hub: { kanban: vi.fn().mockResolvedValue({ next: 1, cards: [] }) } as unknown as HubClient,
      activeProfile: "pessoal",
      activeProject: "game",
      activeSession: "pessoal--game--claude-1",
      sessions: [session("pessoal--game--claude-1", "game", "claude")],
    });
    render(<SessionArea />);
    fireEvent.click(screen.getByRole("button", { name: /Quadro/ }));
    fireEvent.click(screen.getByRole("button", { name: /Editor/ }));
    expect(screen.getByRole("tab", { name: "Quadro" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Editor" })).toBeInTheDocument();
    expect(screen.getByText("editor")).toBeInTheDocument();

    // Trocar para o Quadro e depois para a sessão, sem sumir com as abas.
    fireEvent.click(screen.getByRole("tab", { name: "Quadro" }));
    expect(await screen.findByRole("region", { name: "Coluna Backlog" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Claude 1" }));
    expect(screen.queryByRole("region", { name: "Coluna Backlog" })).toBeNull();
    expect(screen.getByText("terminal pessoal--game--claude-1")).toBeInTheDocument();

    // Fechar a aba do Editor tira ela da barra.
    fireEvent.click(screen.getByTitle("Fechar Editor"));
    expect(screen.queryByRole("tab", { name: "Editor" })).toBeNull();
    expect(screen.getByRole("tab", { name: "Quadro" })).toBeInTheDocument();
  });

  it("com o quadro aberto, as abas continuam e clicar numa volta ao terminal", async () => {
    useAppStore.setState({
      hub: { kanban: vi.fn().mockResolvedValue({ next: 1, cards: [] }) } as unknown as HubClient,
      activeProfile: "pessoal",
      activeProject: "game",
      activeSession: "pessoal--game--claude-1",
      sessions: [session("pessoal--game--claude-1", "game", "claude")],
    });
    render(<SessionArea />);
    fireEvent.click(screen.getByRole("button", { name: /Quadro/ }));
    expect(await screen.findByRole("region", { name: "Coluna Backlog" })).toBeInTheDocument();
    const tab = screen.getByRole("tab", { name: "Claude 1" });
    fireEvent.click(tab);
    expect(screen.queryByRole("region", { name: "Coluna Backlog" })).toBeNull();
    expect(screen.getByText("terminal pessoal--game--claude-1")).toBeInTheDocument();
  });

  it("nomeia a aba pelo agente e número", () => {
    expect(tabLabel(session("pessoal--game--opencode-3", "game", "opencode"))).toBe("OpenCode 3");
  });

  const comPreview = (previewUrl: string | null) =>
    useAppStore.setState({
      hub: {
        kanban: vi.fn().mockResolvedValue({ next: 1, cards: [] }),
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
      } as unknown as HubClient,
      activeProfile: "pessoal",
      activeProject: "game",
      sessions: [session("pessoal--game--claude-1", "game", "claude")],
    });

  it("Visualizar abre o preview do projeto como aba, sem mexer na sessão", async () => {
    comPreview("http://10.77.0.10:5173");
    render(<SessionArea />);

    fireEvent.click(screen.getByRole("button", { name: /Visualizar/ }));
    expect(await screen.findByRole("tab", { name: "Preview" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Claude 1" })).toBeInTheDocument();
    expect(useAppStore.getState().sessions).toHaveLength(1);
  });

  it("sem URL de preview, o Visualizar avisa em vez de abrir", async () => {
    comPreview(null);
    render(<SessionArea />);

    fireEvent.click(screen.getByRole("button", { name: /Visualizar/ }));
    await vi.waitFor(() => expect(useAppStore.getState().toast).toMatch(/URL de preview/));
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
  });

  it("botão Terminal abre um bash fora do jail sem trocar o último agente", async () => {
    const created = session("pessoal--game--shell-1", "game", "shell");
    const createSession = vi.fn().mockResolvedValue(created);
    useAppStore.setState({
      hub: { createSession } as unknown as HubClient,
      activeProfile: "pessoal",
      activeProject: "game",
      lastAgent: "opencode",
    });
    render(<SessionArea />);
    fireEvent.click(screen.getByRole("button", { name: /Terminal/ }));
    expect(createSession).toHaveBeenCalledWith({
      profile: "pessoal",
      project: "game",
      agent: "shell",
    });
    expect(await screen.findByRole("tab", { name: "Terminal 1" })).toBeInTheDocument();
    expect(useAppStore.getState().lastAgent).toBe("opencode");
    expect(tabLabel(created)).toBe("Terminal 1");
  });

  it("perfil sem login do Claude: avisa com o comando e não abre a aba", async () => {
    const createSession = vi.fn();
    useAppStore.setState({
      hub: { createSession } as unknown as HubClient,
      profiles: [
        { id: "trabalho", label: "Trabalho", color: "#0ea5e9", root: "/t", claude_login: false },
      ],
      activeProfile: "trabalho",
      activeProject: "beewell",
    });
    render(<SessionArea />);
    expect(screen.getByText("bin/assistente login claude trabalho")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Claude/ }));
    expect(createSession).not.toHaveBeenCalled();
    expect(useAppStore.getState().actionError).toMatch(/login claude trabalho/);
    // OpenCode e Terminal continuam abrindo.
    fireEvent.click(screen.getByRole("button", { name: /OpenCode/ }));
    expect(createSession).toHaveBeenCalledWith(expect.objectContaining({ agent: "opencode" }));
  });

  it("mostra só as sessões do projeto ativo e abre a última", () => {
    useAppStore.setState({
      activeProfile: "pessoal",
      activeProject: "game",
      sessions: [
        session("pessoal--game--claude-1", "game", "claude"),
        session("pessoal--outro--claude-1", "outro", "claude"),
        session("pessoal--game--opencode-1", "game", "opencode"),
      ],
    });
    render(<SessionArea />);
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "Claude 1",
      "OpenCode 1",
    ]);
    expect(screen.getByText("terminal pessoal--game--opencode-1")).toBeInTheDocument();
  });

  const twoTabs = () =>
    useAppStore.setState({
      activeProfile: "pessoal",
      activeProject: "game",
      sessions: [
        session("pessoal--game--claude-1", "game", "claude"),
        session("pessoal--game--opencode-1", "game", "opencode"),
      ],
      activeSession: "pessoal--game--opencode-1",
    });

  it("Ctrl+Shift+W fecha a aba sem matar a sessão; a seta reabre", () => {
    twoTabs();
    render(<Harness />);

    fireEvent.keyDown(window, { key: "W", ctrlKey: true, shiftKey: true });
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["Claude 1"]);
    // A sessão continua viva (fechar aba não chama o DELETE do hubd).
    expect(useAppStore.getState().sessions).toHaveLength(2);

    fireEvent.click(screen.getByTitle(/Reabrir OpenCode 1/));
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual([
      "Claude 1",
      "OpenCode 1",
    ]);
    expect(screen.getByText("terminal pessoal--game--opencode-1")).toBeInTheDocument();
  });

  it("Ctrl+Tab alterna para a próxima aba", () => {
    twoTabs();
    render(<Harness />);

    fireEvent.keyDown(window, { key: "Tab", ctrlKey: true });
    expect(useAppStore.getState().activeSession).toBe("pessoal--game--claude-1");
    expect(screen.getByText("terminal pessoal--game--claude-1")).toBeInTheDocument();
  });

  it("com todas as abas fechadas, oferece reabrir em vez do terminal", () => {
    twoTabs();
    useAppStore.setState({
      closedTabs: ["pessoal--game--claude-1", "pessoal--game--opencode-1"],
    });
    render(<Harness />);
    expect(screen.getByText("Todas as abas fechadas")).toBeInTheDocument();
    fireEvent.click(screen.getByTitle(/Reabrir/));
    expect(screen.getByText("terminal pessoal--game--opencode-1")).toBeInTheDocument();
  });
});
