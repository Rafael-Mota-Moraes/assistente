import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HubClient } from "../../bridge/hub";
import type { KanbanBoard as Board, KanbanCard, SessionInfo } from "../../bridge/types";
import { useAppStore, visibleProjectSessions } from "../../store";
import { KanbanBoard } from "./KanbanBoard";

// O terminal de verdade (xterm) não roda no jsdom: o teste só confere qual sessão ele recebe.
vi.mock("../sessions/TerminalView", () => ({
  TerminalView: ({ sessionId }: { sessionId: string }) => <div>terminal {sessionId}</div>,
}));

const card = (id: string, extra: Partial<KanbanCard> = {}): KanbanCard => ({
  id,
  title: `Tarefa ${id}`,
  description: "",
  priority: "normal",
  kind: "manual",
  column: "backlog",
  created_at: 0,
  ...extra,
});

const claude: SessionInfo = {
  id: "pessoal--game--claude-1",
  profile: "pessoal",
  project: "game",
  agent: "claude",
  created_at: 0,
  attached: 1,
  dead: false,
};

function connect(board: Board) {
  const hub = {
    kanban: vi.fn().mockResolvedValue(board),
    createCard: vi.fn().mockResolvedValue(card("T9")),
    updateCard: vi.fn().mockResolvedValue(card("T1")),
    deleteCard: vi.fn().mockResolvedValue(undefined),
    mandarCard: vi.fn().mockResolvedValue(claude),
    restartWorker: vi.fn().mockResolvedValue(claude),
    explorarBacklog: vi.fn().mockResolvedValue(claude),
    deleteSession: vi.fn().mockResolvedValue(undefined),
    setOrchestrator: vi.fn().mockResolvedValue(board),
  };
  useAppStore.setState({
    hub: hub as unknown as HubClient,
    activeProfile: "pessoal",
    activeProject: "game",
    sessions: [claude],
  });
  render(<KanbanBoard />);
  return hub;
}

const column = (name: string) => screen.findByRole("region", { name: `Coluna ${name}` });

describe("KanbanBoard", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("mostra as três colunas; só o card manual do backlog tem Mandar", async () => {
    connect({
      next: 3,
      cards: [
        card("T1"),
        card("T2", { kind: "agente" }),
        card("T3", { column: "fazendo", holder: claude.id }),
        card("T4", { column: "feito", commit: "abc1234def" }),
      ],
    });
    const backlog = await column("Backlog");
    expect(within(backlog).getByRole("button", { name: /Mandar/ })).toBeInTheDocument();
    expect(within(backlog).getAllByRole("button", { name: /Mandar/ })).toHaveLength(1);
    const fazendo = await column("Fazendo");
    expect(within(fazendo).getByText("em Claude 1")).toBeInTheDocument();
    expect(within(fazendo).getByRole("button", { name: /Reiniciar/ })).toBeInTheDocument();
    const feito = await column("Feito");
    expect(within(feito).getByText("abc1234")).toBeInTheDocument();
  });

  it("Mandar envia a tarefa manual para um trabalhador", async () => {
    const hub = connect({ next: 2, cards: [card("T1")] });
    const backlog = await column("Backlog");
    fireEvent.click(within(backlog).getByRole("button", { name: /Mandar/ }));
    await vi.waitFor(() => expect(hub.mandarCard).toHaveBeenCalledWith("pessoal", "game", "T1"));
  });

  it("arrastar para outra coluna muda a coluna do card", async () => {
    const hub = connect({ next: 2, cards: [card("T1")] });
    const source = await screen.findByRole("article", { name: "T1 Tarefa T1" });
    const feito = await column("Feito");
    const dataTransfer = { setData: vi.fn(), getData: vi.fn() };
    fireEvent.dragStart(source, { dataTransfer });
    fireEvent.dragOver(feito, { dataTransfer });
    fireEvent.drop(feito, { dataTransfer });
    await vi.waitFor(() =>
      expect(hub.updateCard).toHaveBeenCalledWith("pessoal", "game", "T1", { column: "feito" }),
    );
  });

  it("Novo card cria com tipo e prioridade escolhidos", async () => {
    const hub = connect({ next: 1, cards: [] });
    fireEvent.click(await screen.findByRole("button", { name: /Novo card/ }));
    const dialog = screen.getByRole("dialog", { name: "Novo card" });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Título" }), {
      target: { value: "  Tela de login  " },
    });
    fireEvent.click(within(dialog).getByRole("radio", { name: /Para o agente/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Salvar" }));
    await vi.waitFor(() =>
      expect(hub.createCard).toHaveBeenCalledWith("pessoal", "game", {
        title: "  Tela de login  ",
        description: "",
        priority: "normal",
        kind: "agente",
      }),
    );
  });

  it("Reiniciar relança o trabalhador que segura a tarefa", async () => {
    const hub = connect({
      next: 2,
      cards: [card("T1", { column: "fazendo", holder: claude.id })],
    });
    const fazendo = await column("Fazendo");
    fireEvent.click(within(fazendo).getByRole("button", { name: /Reiniciar/ }));
    await vi.waitFor(() => expect(hub.restartWorker).toHaveBeenCalledWith(claude.id));
  });

  it("Popular pelo backlog mostra o chat dentro do quadro, sem trocar de aba", async () => {
    // A sessão nova ainda não está na lista (ela chega no próximo polling): é o caso real.
    const hub = connect({ next: 1, cards: [] });
    useAppStore.setState({ sessions: [], activeView: { key: "pessoal/game", kind: "board" } });
    fireEvent.click(await screen.findByRole("button", { name: /Popular pelo backlog/ }));
    await vi.waitFor(() => expect(hub.explorarBacklog).toHaveBeenCalledWith("pessoal", "game"));
    expect(await screen.findByText(`terminal ${claude.id}`)).toBeInTheDocument();
    const state = useAppStore.getState();
    expect(state.boardChat).toEqual({ key: "pessoal/game", sessionId: claude.id });
    expect(state.activeView).toEqual({ key: "pessoal/game", kind: "board" });
    // Fora das abas: o chat não aparece como sessão do projeto.
    expect(visibleProjectSessions(state)).not.toContainEqual(claude);
  });

  it("Encerrar o chat tira o terminal do quadro e apaga a sessão", async () => {
    const hub = connect({ next: 1, cards: [] });
    useAppStore.setState({ boardChat: { key: "pessoal/game", sessionId: claude.id } });
    fireEvent.click(await screen.findByRole("button", { name: /Encerrar o chat/ }));
    await vi.waitFor(() => expect(hub.deleteSession).toHaveBeenCalledWith(claude.id));
    expect(useAppStore.getState().boardChat).toBeNull();
    expect(screen.queryByText(`terminal ${claude.id}`)).toBeNull();
  });

  it("sem login do Claude no perfil, não abre a sessão e mostra o comando de login", async () => {
    const hub = connect({ next: 1, cards: [] });
    useAppStore.setState({
      profiles: [
        { id: "pessoal", label: "Pessoal", color: "#fff", root: "/x", claude_login: false },
      ],
    });
    fireEvent.click(await screen.findByRole("button", { name: /Popular pelo backlog/ }));
    expect(await screen.findByText(/ainda não tem login no perfil Pessoal/)).toBeInTheDocument();
    expect(hub.explorarBacklog).not.toHaveBeenCalled();
  });

  it("Parar encerra o trabalhador que segura o card (a tarefa volta ao backlog pela leitura)", async () => {
    const hub = connect({ next: 2, cards: [card("T3", { column: "fazendo", holder: claude.id })] });
    const fazendo = await column("Fazendo");
    fireEvent.click(within(fazendo).getByRole("button", { name: /Parar/ }));
    await waitFor(() => expect(hub.deleteSession).toHaveBeenCalledWith(claude.id));
  });

  it("Mandar mostra que está processando e o erro continua na tela depois do poll", async () => {
    const hub = connect({ next: 2, cards: [card("T1")] });
    let fail: (reason: Error) => void = () => undefined;
    hub.mandarCard.mockReturnValueOnce(
      new Promise((_, reject) => {
        fail = reject;
      }),
    );
    const backlog = await column("Backlog");
    fireEvent.click(within(backlog).getByRole("button", { name: /Mandar/ }));
    expect(await screen.findByRole("status")).toHaveTextContent("Processando");
    fail(new Error("não consegui rodar `opencode models`"));
    expect(await screen.findByText(/não consegui rodar/)).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    // O poll roda a cada 2 s: o erro da ação não pode sumir com ele.
    const reads = hub.kanban.mock.calls.length;
    await waitFor(() => expect(hub.kanban.mock.calls.length).toBeGreaterThan(reads), {
      timeout: 4000,
    });
    expect(screen.getByText(/não consegui rodar/)).toBeInTheDocument();
  });

  it("liga o orquestrador, avisa o que abriu e mostra o erro dele", async () => {
    const board: Board = { next: 2, cards: [card("T1", { kind: "agente" })] };
    const hub = connect(board);
    const on = { enabled: true, max_workers: 1 };
    // O PUT demora (ligar já abre trabalhadores): o botão diz o que está fazendo.
    let answer: (b: Board) => void = () => undefined;
    hub.setOrchestrator.mockReturnValueOnce(new Promise<Board>((r) => (answer = r)));
    fireEvent.click(await screen.findByRole("switch", { name: /Orquestrador desligado/ }));
    expect(await screen.findByRole("switch", { name: /Ligando…/ })).toBeInTheDocument();
    expect(hub.setOrchestrator).toHaveBeenCalledWith("pessoal", "game", {
      enabled: true,
      max_workers: 1,
    });
    hub.kanban.mockResolvedValue({ ...board, orchestrator: on });
    answer({ ...board, orchestrator: on, started: ["T1"] });
    await waitFor(() =>
      expect(useAppStore.getState().toast).toBe(
        "Orquestrador ligado (até 1 em fazendo). T1 foi para trabalhador.",
      ),
    );
    expect(await screen.findByRole("switch", { name: /Orquestrador ligado/ })).toBeInTheDocument();

    hub.setOrchestrator.mockResolvedValueOnce({
      ...board,
      orchestrator: { ...on, max_workers: 3 },
    });
    fireEvent.change(screen.getByRole("combobox", { name: "Trabalhadores ao mesmo tempo" }), {
      target: { value: "3" },
    });
    await waitFor(() =>
      expect(useAppStore.getState().toast).toBe(
        "Limite: até 3 em fazendo. Nenhum card de agente livre no backlog agora; ele confere de novo a cada 20 s.",
      ),
    );

    hub.kanban.mockResolvedValue({
      ...board,
      orchestrator: on,
      orchestrator_error: "T1: nenhum modelo grátis",
    });
    // O erro chega pelo poll seguinte (2 s).
    expect(
      await screen.findByText("Orquestrador: T1: nenhum modelo grátis", {}, { timeout: 4000 }),
    ).toBeInTheDocument();
  });

  it("instruções dos trabalhadores vão para o orquestrador sem mexer no resto", async () => {
    const board: Board = {
      next: 1,
      cards: [],
      orchestrator: { enabled: true, max_workers: 2 },
    };
    const hub = connect(board);
    fireEvent.click(await screen.findByRole("button", { name: "Instruções dos trabalhadores" }));
    const dialog = await screen.findByRole("dialog", { name: "Instruções dos trabalhadores" });
    hub.setOrchestrator.mockResolvedValueOnce({
      ...board,
      orchestrator: { enabled: true, max_workers: 2, instructions: "Use pnpm." },
    });
    fireEvent.change(within(dialog).getByRole("textbox"), { target: { value: "Use pnpm." } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Salvar" }));
    await waitFor(() =>
      expect(hub.setOrchestrator).toHaveBeenCalledWith("pessoal", "game", {
        enabled: true,
        max_workers: 2,
        instructions: "Use pnpm.",
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Instruções dos trabalhadores" })).toBeNull(),
    );
    expect(useAppStore.getState().toast).toBe(
      "Instruções salvas: valem para os próximos trabalhadores deste projeto.",
    );
  });

  it("card que voltou do fazendo pede Tentar de novo", async () => {
    const hub = connect({
      next: 3,
      cards: [card("T1", { kind: "agente", failures: 2 }), card("T2", { failures: 1 })],
    });
    const backlog = await column("Backlog");
    expect(within(backlog).getAllByText(/Voltou .*× sem concluir/)).toHaveLength(2);
    const retry = within(backlog).getAllByRole("button", { name: /Tentar de novo/ });
    expect(retry).toHaveLength(1);
    fireEvent.click(retry[0]);
    await waitFor(() =>
      expect(hub.updateCard).toHaveBeenCalledWith("pessoal", "game", "T1", { failures: 0 }),
    );
  });
});
