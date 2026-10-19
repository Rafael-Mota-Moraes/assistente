import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SessionInfo } from "../../bridge/types";
import { useAppStore } from "../../store";
import { SessionArea, tabLabel } from "./SessionArea";

vi.mock("./TerminalView", () => ({
  TerminalView: ({ sessionId }: { sessionId: string }) => <div>terminal {sessionId}</div>,
}));

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
});
