import { describe, expect, it, vi } from "vitest";
import { hostAutoExec, hostExec } from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import type { ExecView } from "../../bridge/types";
import { approveOnHost, autoRunOnHost } from "./hostExec";

vi.mock("../../bridge/host", () => ({
  hostExec: vi.fn(),
  hostAutoExec: vi.fn(),
}));

const request = (id: string): ExecView => ({
  id,
  session: "pessoal--game--claude-1",
  profile: "pessoal",
  project: "game",
  cwd: "/raiz/game",
  command: "git push",
  timeout_secs: 600,
  created_at: 0,
  state: "pending",
  target: "host",
  auto_approved: false,
  result: null,
});

function fakeHub() {
  return {
    approveExec: vi.fn().mockResolvedValue(undefined),
    execResult: vi.fn().mockResolvedValue(undefined),
  };
}

describe("approveOnHost", () => {
  it("aprova, roda no PC e devolve o resultado ao hubd", async () => {
    const result = {
      exit_code: 0,
      stdout: "push ok\n",
      stderr: "",
      truncated: false,
      timed_out: false,
      duration_ms: 12,
    };
    vi.mocked(hostExec).mockResolvedValue(result);
    const hub = fakeHub();
    await approveOnHost(request("p1"), hub as unknown as HubClient);
    expect(hub.approveExec).toHaveBeenCalledWith("p1");
    expect(hostExec).toHaveBeenCalledWith("p1");
    expect(hub.execResult).toHaveBeenCalledWith("p1", result);
  });

  it("falha do PC volta ao hubd como erro: o agente não fica esperando", async () => {
    vi.mocked(hostExec).mockRejectedValue(new Error("não consegui iniciar o bash"));
    const hub = fakeHub();
    await expect(approveOnHost(request("p2"), hub as unknown as HubClient)).rejects.toThrow(
      "não consegui iniciar o bash",
    );
    expect(hub.execResult).toHaveBeenCalledWith("p2", { error: "não consegui iniciar o bash" });
  });

  it("lista do PC (F9.11): fora da lista não roda nem responde ao hubd", async () => {
    vi.mocked(hostAutoExec).mockResolvedValue(null);
    const hub = fakeHub();
    expect(await autoRunOnHost(request("p3"), hub as unknown as HubClient)).toBe(false);
    expect(hub.approveExec).not.toHaveBeenCalled();
    expect(hub.execResult).not.toHaveBeenCalled();
  });

  it("lista do PC (F9.11): na lista, o app roda e o resultado volta ao hubd", async () => {
    const result = {
      exit_code: 0,
      stdout: "",
      stderr: "",
      truncated: false,
      timed_out: false,
      duration_ms: 3,
    };
    vi.mocked(hostAutoExec).mockResolvedValue(result);
    const hub = fakeHub();
    expect(await autoRunOnHost(request("p4"), hub as unknown as HubClient)).toBe(true);
    expect(hub.approveExec).not.toHaveBeenCalled();
    expect(hub.execResult).toHaveBeenCalledWith("p4", result);
  });
});
