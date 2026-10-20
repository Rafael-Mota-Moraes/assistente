import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { openInCursor } from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import type { ProfileView, Project, SessionInfo } from "../../bridge/types";
import { useAppStore } from "../../store";
import { ProfileSwitcher } from "../profiles/ProfileSwitcher";
import { ProjectList } from "./ProjectList";

vi.mock("../../bridge/host", () => ({
  openInCursor: vi.fn(async () => {}),
}));

const project = (name: string, has_project_config = false): Project => ({
  name,
  path: `/raiz/${name}`,
  has_project_config,
});

const session = (project: string, profile = "pessoal"): SessionInfo => ({
  id: `${profile}--${project}--claude-1`,
  profile,
  project,
  agent: "claude",
  created_at: 0,
  attached: 0,
});

const profiles: ProfileView[] = [
  { id: "pessoal", label: "Pessoal", color: "#8b5cf6", root: "/raiz/pessoal" },
  { id: "trabalho", label: "Trabalho", color: "#0ea5e9", root: "/raiz/trabalho" },
];

/** HubClient de mentira: devolve os projetos do perfil pedido e registra as chamadas. */
function fakeHub(byProfile: Record<string, Project[]>) {
  const projects = vi.fn(async (profile: string) => byProfile[profile] ?? []);
  return { hub: { projects } as unknown as HubClient, projects };
}

function connect(hub: HubClient, extra: Partial<ReturnType<typeof useAppStore.getState>> = {}) {
  useAppStore.setState({
    hub,
    health: {
      ok: true,
      version: "0.1.0",
      uptime_seconds: 1,
      commit: { hubd: "abc1234", ui: null },
    },
    profiles,
    activeProfile: "pessoal",
    ...extra,
  });
}

describe("ProjectList", () => {
  it("lista os projetos do perfil ativo com contagem de sessões e o 🌙", async () => {
    const { hub } = fakeHub({ pessoal: [project("game", true), project("estudos")] });
    connect(hub, {
      sessions: [
        session("game"),
        session("game"),
        session("estudos"),
        // Sessão de outro perfil com o mesmo nome de projeto não entra na conta.
        session("game", "trabalho"),
      ],
    });
    render(<ProjectList />);

    expect(await screen.findByText("game")).toBeInTheDocument();
    expect(screen.getByText("estudos")).toBeInTheDocument();
    expect(screen.getByTitle("2 sessão(ões) aberta(s)")).toHaveTextContent("2");
    expect(screen.getByTitle("1 sessão(ões) aberta(s)")).toHaveTextContent("1");
    expect(screen.queryByTitle("3 sessão(ões) aberta(s)")).not.toBeInTheDocument();
    expect(screen.getByLabelText("pronto p/ modo dormir")).toBeInTheDocument();
    expect(hub.projects).toHaveBeenCalled();
  });

  it("trocar de perfil troca a lista sem misturar", async () => {
    const { hub, projects } = fakeHub({ pessoal: [project("game")], trabalho: [project("erp")] });
    connect(hub);
    render(
      <>
        <ProfileSwitcher />
        <ProjectList />
      </>,
    );

    expect(await screen.findByText("game")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Trabalho" }));

    expect(await screen.findByText("erp")).toBeInTheDocument();
    expect(screen.queryByText("game")).not.toBeInTheDocument();
    expect(projects).toHaveBeenLastCalledWith("trabalho");
  });

  it("raiz sem repositórios mostra o recado", async () => {
    const { hub } = fakeHub({ pessoal: [] });
    connect(hub);
    render(<ProjectList />);
    expect(await screen.findByText(/Nenhum repositório git/)).toBeInTheDocument();
  });

  it("clicar no projeto ativa e o botão abre no Cursor", async () => {
    const { hub } = fakeHub({ pessoal: [project("game")] });
    connect(hub);
    render(<ProjectList />);

    fireEvent.click(await screen.findByText("game"));
    expect(useAppStore.getState().activeProject).toBe("game");

    fireEvent.click(screen.getByTitle("Abrir no Cursor"));
    expect(openInCursor).toHaveBeenCalledWith("/raiz/game");
  });

  it("falha ao abrir o Cursor vira toast em vez de silêncio", async () => {
    vi.mocked(openInCursor).mockRejectedValueOnce(
      new Error("não consegui abrir o Cursor: program not found"),
    );
    const { hub } = fakeHub({ pessoal: [project("game")] });
    connect(hub);
    render(<ProjectList />);

    fireEvent.click(await screen.findByText("game"));
    fireEvent.click(screen.getByTitle("Abrir no Cursor"));
    await waitFor(() =>
      expect(useAppStore.getState().toast).toBe("não consegui abrir o Cursor: program not found"),
    );
  });
});
