import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SessionInfo } from "../../bridge/types";
import { useAppStore } from "../../store";
import { SessionArea, tabLabel } from "./SessionArea";
import { useShortcuts } from "./shortcuts";

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
  it("nomeia a aba pelo agente e número", () => {
    expect(tabLabel(session("pessoal--game--opencode-3", "game", "opencode"))).toBe("OpenCode 3");
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
