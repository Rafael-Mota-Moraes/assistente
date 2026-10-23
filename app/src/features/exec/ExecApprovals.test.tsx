import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { hostAutoExec, hostExec } from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import type { ExecResult, ExecView, Health } from "../../bridge/types";
import { useAppStore } from "../../store";
import { ExecApprovals } from "./ExecApprovals";

vi.mock("../../bridge/host", () => ({
  hostExec: vi.fn(),
  hostAutoExec: vi.fn(),
}));

const ok = (stdout = ""): ExecResult => ({
  exit_code: 0,
  stdout,
  stderr: "",
  truncated: false,
  timed_out: false,
  duration_ms: 5,
});

const request = (id: string, extra: Partial<ExecView> = {}): ExecView => ({
  id,
  session: "pessoal--game--claude-1",
  profile: "pessoal",
  project: "game",
  cwd: "/raiz/game",
  command: "./dev just check",
  timeout_secs: 600,
  created_at: 0,
  state: "pending",
  target: "vm",
  auto_approved: false,
  result: null,
  ...extra,
});

function connect(requests: ExecView[], { pcList = false } = {}) {
  // Fora da lista do PC (F9.11), por padrão: o card espera o clique.
  vi.mocked(hostAutoExec).mockImplementation(async (id) => {
    const request = requests.find((r) => r.id === id);
    return pcList && request ? ok(`auto ${request.command}`) : null;
  });
  const hub = {
    execRequests: vi.fn().mockResolvedValue(requests),
    approveExec: vi.fn().mockResolvedValue(undefined),
    denyExec: vi.fn().mockResolvedValue(undefined),
    execResult: vi.fn().mockResolvedValue(undefined),
  };
  useAppStore.setState({
    hub: hub as unknown as HubClient,
    health: { ok: true } as Health,
    sessions: [
      {
        id: "pessoal--game--claude-1",
        profile: "pessoal",
        project: "game",
        agent: "claude",
        created_at: 0,
        attached: 1,
        dead: false,
      },
    ],
  });
  render(<ExecApprovals />);
  return hub;
}

describe("ExecApprovals", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("mostra o pedido com comando, origem e pasta; Aprovar roda", async () => {
    const hub = connect([request("p1")]);
    const card = await screen.findByRole("region", { name: /Pedido de game · Claude 1/ });
    expect(within(card).getByText("./dev just check")).toBeInTheDocument();
    expect(within(card).getByText(/em \/raiz\/game/)).toBeInTheDocument();
    fireEvent.click(within(card).getByRole("button", { name: "Aprovar" }));
    await vi.waitFor(() => expect(hub.approveExec).toHaveBeenCalledWith("p1", false));
  });

  it("Negar nega", async () => {
    const hub = connect([request("p1")]);
    fireEvent.click(await screen.findByRole("button", { name: "Negar" }));
    await vi.waitFor(() => expect(hub.denyExec).toHaveBeenCalledWith("p1"));
  });

  it("'sempre permitir' avisa da fuga por arquivos do projeto antes de gravar", async () => {
    const confirm = vi
      .spyOn(window, "confirm")
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true);
    const hub = connect([request("p1")]);
    const always = await screen.findByRole("button", { name: /sempre permitir/ });
    fireEvent.click(always);
    expect(confirm.mock.calls[0]?.[0]).toMatch(/pode editá-los/);
    expect(hub.approveExec).not.toHaveBeenCalled();
    fireEvent.click(always);
    await vi.waitFor(() => expect(hub.approveExec).toHaveBeenCalledWith("p1", true));
  });

  it("pedido para o PC (F9.9/F9.10) mostra 'no PC', não oferece 'sempre permitir' e roda no host", async () => {
    vi.mocked(hostExec).mockResolvedValue({
      exit_code: 0,
      stdout: "push ok\n",
      stderr: "",
      truncated: false,
      timed_out: false,
      duration_ms: 5,
    });
    const hub = connect([request("h1", { target: "host", command: "git push" })]);
    const card = await screen.findByRole("region", { name: /Pedido de game · Claude 1/ });
    expect(within(card).getByText(/no PC/)).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: /sempre permitir/ })).not.toBeInTheDocument();
    const approve = within(card).getByRole("button", { name: "Aprovar" });
    await vi.waitFor(() => expect(approve).toBeEnabled());
    fireEvent.click(approve);
    // O hubd só marca como rodando; quem executa no PC é o app, e o resultado volta.
    await vi.waitFor(() => expect(hostExec).toHaveBeenCalledWith("h1"));
    await vi.waitFor(() =>
      expect(hub.execResult).toHaveBeenCalledWith(
        "h1",
        expect.objectContaining({ stdout: "push ok\n" }),
      ),
    );
  });

  it("não aprova duas vezes o mesmo pedido do PC (F9.10)", async () => {
    let done!: (value: ExecResult) => void;
    vi.mocked(hostExec).mockReturnValue(
      new Promise<ExecResult>((resolve) => {
        done = resolve;
      }),
    );
    connect([request("h2", { target: "host", command: "git push" })]);
    const card = await screen.findByRole("region", { name: /Pedido de game · Claude 1/ });
    const approve = within(card).getByRole("button", { name: "Aprovar" });
    await vi.waitFor(() => expect(approve).toBeEnabled());
    fireEvent.click(approve);
    await vi.waitFor(() => expect(hostExec).toHaveBeenCalledTimes(1));
    fireEvent.click(approve);
    expect(hostExec).toHaveBeenCalledTimes(1);
    done({
      exit_code: 0,
      stdout: "",
      stderr: "",
      truncated: false,
      timed_out: false,
      duration_ms: 1,
    });
  });

  it("card do PC (F9.11): em vermelho, 'vai rodar no seu PC', comando e pasta por inteiro", async () => {
    const cwd = "/home/pcdorafa/Documentos/pessoal/projetos/um-projeto-com-nome-bem-comprido/sub";
    const command = "git push origin agente/T3 && git push --tags origin";
    connect([request("h3", { target: "host", command, cwd })]);
    const card = await screen.findByRole("region", { name: /Pedido de game · Claude 1/ });
    expect(card.className).toMatch(/border-red-/);
    const title = within(card).getByText("Vai rodar no seu PC");
    expect(title.className).toMatch(/text-red-100/);
    expect(within(card).queryByText(/text-red-100/)).not.toBeInTheDocument();
    const shown = within(card).getByText(command);
    expect(shown.className).not.toMatch(/max-h/);
    const folder = within(card).getByText(new RegExp(`em ${cwd}`));
    expect(folder.className).not.toMatch(/truncate/);
    expect(folder.className).toMatch(/break-all/);
  });

  it("pedido da VM continua âmbar e não passa pela lista do PC", async () => {
    connect([request("v1")]);
    const card = await screen.findByRole("region", { name: /Pedido de game · Claude 1/ });
    expect(card.className).toMatch(/border-amber-/);
    expect(within(card).getByText("Rodar fora do jail?")).toBeInTheDocument();
    expect(hostAutoExec).not.toHaveBeenCalled();
  });

  it("lista do PC (F9.11): comando na lista roda sem clique, uma vez só", async () => {
    vi.mocked(hostAutoExec).mockClear();
    const hub = connect([request("h4", { target: "host", command: "git push" })], {
      pcList: true,
    });
    await vi.waitFor(() =>
      expect(hub.execResult).toHaveBeenCalledWith(
        "h4",
        expect.objectContaining({ stdout: "auto git push" }),
      ),
    );
    // Quem confere a lista e aprova é o app no host; a UI não aprova nem roda à parte.
    expect(hostAutoExec).toHaveBeenCalledTimes(1);
    expect(hub.approveExec).not.toHaveBeenCalled();
    expect(hostExec).not.toHaveBeenCalled();
  });

  it("lista do PC (F9.11): fora da lista, nada roda até o clique", async () => {
    vi.mocked(hostExec).mockClear();
    vi.mocked(hostExec).mockResolvedValue(ok("push ok\n"));
    const hub = connect([request("h5", { target: "host", command: "git push --force" })]);
    const card = await screen.findByRole("region", { name: /Pedido de game · Claude 1/ });
    await vi.waitFor(() => expect(hostAutoExec).toHaveBeenCalledWith("h5"));
    const approve = within(card).getByRole("button", { name: "Aprovar" });
    await vi.waitFor(() => expect(approve).toBeEnabled());
    expect(hub.approveExec).not.toHaveBeenCalled();
    expect(hostExec).not.toHaveBeenCalled();
    expect(hub.execResult).not.toHaveBeenCalled();
    fireEvent.click(approve);
    await vi.waitFor(() => expect(hostExec).toHaveBeenCalledWith("h5"));
    expect(hub.approveExec).toHaveBeenCalledWith("h5");
  });

  it("lista do PC: falha depois de aprovar volta ao hubd como erro e avisa", async () => {
    const hub = connect([request("h6", { target: "host", command: "git push" })]);
    vi.mocked(hostAutoExec).mockRejectedValue(new Error("o comando mudou"));
    await vi.waitFor(() =>
      expect(hub.execResult).toHaveBeenCalledWith("h6", { error: "o comando mudou" }),
    );
    await vi.waitFor(() => expect(useAppStore.getState().toast).toBe("o comando mudou"));
  });

  it("recentes ficam recolhidos e mostram o resultado", async () => {
    connect([
      request("d1", {
        state: "done",
        command: "cargo build",
        result: {
          exit_code: 101,
          stdout: "",
          stderr: "erro",
          truncated: false,
          timed_out: false,
          duration_ms: 10,
        },
      }),
      request("d2", { state: "denied", command: "rm -rf build" }),
    ]);
    const toggle = await screen.findByRole("button", { name: /2 recentes/ });
    expect(screen.queryByText("cargo build")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    expect(screen.getByText("cargo build")).toBeInTheDocument();
    expect(screen.getByText("código 101")).toBeInTheDocument();
    expect(screen.getByText("negado")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Aprovar" })).not.toBeInTheDocument();
  });

  it("sem pedidos, nada aparece", async () => {
    const hub = connect([]);
    await vi.waitFor(() => expect(hub.execRequests).toHaveBeenCalled());
    expect(screen.queryByLabelText("Pedidos fora do jail")).not.toBeInTheDocument();
  });
});
