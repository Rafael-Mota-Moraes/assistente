import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { hostExec } from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import type { ExecResult, ExecView, Health } from "../../bridge/types";
import { useAppStore } from "../../store";
import { ExecApprovals } from "./ExecApprovals";

vi.mock("../../bridge/host", () => ({
  hostExec: vi.fn(),
}));

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

function connect(requests: ExecView[]) {
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
    fireEvent.click(within(card).getByRole("button", { name: "Aprovar" }));
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
