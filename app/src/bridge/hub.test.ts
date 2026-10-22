import { describe, expect, it, vi } from "vitest";
import { HubClient, HubError } from "./hub";

function fakeFetch(status: number, body: unknown) {
  return vi.fn<typeof fetch>(async () => new Response(JSON.stringify(body), { status }));
}

describe("HubClient", () => {
  it("manda o token como Bearer", async () => {
    const fetchFn = fakeFetch(200, []);
    const hub = new HubClient({ url: "http://vm:7878", token: "s3gredo" }, fetchFn);
    await hub.profiles();
    expect(fetchFn).toHaveBeenCalledWith("http://vm:7878/api/profiles", {
      headers: { Authorization: "Bearer s3gredo" },
    });
  });

  it("transforma erro da API em HubError com a mensagem do hubd", async () => {
    const hub = new HubClient(
      { url: "http://vm:7878", token: null },
      fakeFetch(401, { error: "token ausente ou inválido" }),
    );
    await expect(hub.profiles()).rejects.toEqual(new HubError(401, "token ausente ou inválido"));
  });

  it("pedidos fora do jail: lista, aprova (com 'sempre') e nega", async () => {
    const fetchFn = vi.fn<typeof fetch>(async (url) =>
      String(url).endsWith("/api/exec")
        ? new Response(JSON.stringify([]), { status: 200 })
        : new Response(null, { status: 204 }),
    );
    const hub = new HubClient({ url: "http://vm:7878", token: null }, fetchFn);
    await expect(hub.execRequests()).resolves.toEqual([]);
    await hub.approveExec("a b", true);
    expect(fetchFn.mock.calls[1]?.[0]).toBe("http://vm:7878/api/exec/a%20b/approve");
    expect(fetchFn.mock.calls[1]?.[1]).toMatchObject({
      method: "POST",
      body: JSON.stringify({ always: true }),
    });
    await hub.denyExec("x");
    expect(fetchFn.mock.calls[2]?.[0]).toBe("http://vm:7878/api/exec/x/deny");
    expect(fetchFn.mock.calls[2]?.[1]).toMatchObject({ method: "POST" });
  });

  it("pedido do PC: o app pergunta o que foi aprovado e devolve o resultado (F9.10)", async () => {
    const fetchFn = vi.fn<typeof fetch>(async (url) =>
      String(url).includes("/result")
        ? new Response(null, { status: 204 })
        : new Response(JSON.stringify({ id: "p1", target: "host", state: "running" }), {
            status: 200,
          }),
    );
    const hub = new HubClient({ url: "http://vm:7878", token: null }, fetchFn);
    const request = await hub.execRequest("a b");
    expect(fetchFn.mock.calls[0]?.[0]).toBe("http://vm:7878/api/exec/a%20b");
    expect(request.target).toBe("host");

    const result = {
      exit_code: 0,
      stdout: "push ok\n",
      stderr: "",
      truncated: false,
      timed_out: false,
      duration_ms: 3,
    };
    await expect(hub.execResult("a b", result)).resolves.toBeUndefined();
    expect(fetchFn.mock.calls[1]?.[0]).toBe("http://vm:7878/api/exec/a%20b/result");
    expect(fetchFn.mock.calls[1]?.[1]).toMatchObject({
      method: "POST",
      body: JSON.stringify(result),
    });
  });

  it("configurações do projeto: GET e PUT no caminho escapado", async () => {
    const view = {
      settings: { docker: true, allow_hosts: [], full_network: false, allowed_commands: [] },
      docker_from_project_file: false,
    };
    const fetchFn = fakeFetch(200, view);
    const hub = new HubClient({ url: "", token: null }, fetchFn);
    await hub.projectSettings("pessoal", "Meu.App");
    expect(fetchFn.mock.calls[0]?.[0]).toBe("/api/projects/pessoal/Meu.App/settings");
    await hub.saveProjectSettings("pessoal", "a b", view.settings);
    expect(fetchFn.mock.calls[1]?.[0]).toBe("/api/projects/pessoal/a%20b/settings");
    expect(fetchFn.mock.calls[1]?.[1]).toMatchObject({ method: "PUT" });
  });

  it("escapa o id do perfil na query", async () => {
    const fetchFn = fakeFetch(200, []);
    await new HubClient({ url: "", token: null }, fetchFn).projects("a b");
    expect(fetchFn.mock.calls[0]?.[0]).toBe("/api/projects?profile=a%20b");
  });

  it("cria sessão com POST JSON e encerra com DELETE (204 sem corpo)", async () => {
    const fetchFn = vi.fn<typeof fetch>(async (_url, init) =>
      init?.method === "DELETE"
        ? new Response(null, { status: 204 })
        : new Response(JSON.stringify({ id: "pessoal--x--claude-1" }), { status: 201 }),
    );
    const hub = new HubClient({ url: "http://vm:7878", token: "t" }, fetchFn);
    const created = await hub.createSession({ profile: "pessoal", project: "x", agent: "claude" });
    expect(created.id).toBe("pessoal--x--claude-1");
    expect(fetchFn.mock.calls[0]?.[1]).toMatchObject({
      method: "POST",
      body: JSON.stringify({ profile: "pessoal", project: "x", agent: "claude" }),
    });
    await expect(hub.deleteSession("pessoal--x--claude-1")).resolves.toBeUndefined();
  });

  it("recolher e mesclar o modo dormir fazem POST com o pedido", async () => {
    const fetchFn = fakeFetch(200, {
      base: "main",
      sleep_branch: "dorme/2026-10-20",
      commits: [{ hash: "abc1234", subject: "merge: f9.9 (modo-dormir)" }],
      relatorio: null,
      fast_forward: true,
    });
    const hub = new HubClient({ url: "http://vm:7878", token: null }, fetchFn);
    const request = { profile: "pessoal", project: "game", date: "2026-10-20" };

    const report = await hub.collectSleep(request);
    expect(fetchFn.mock.calls[0]?.[0]).toBe("http://vm:7878/api/sleep/collect");
    expect(report.commits).toHaveLength(1);
    expect(report.fast_forward).toBe(true);

    await expect(hub.mergeSleep(request)).resolves.toBeUndefined();
    expect(fetchFn.mock.calls[1]?.[0]).toBe("http://vm:7878/api/sleep/merge");
    await expect(hub.stopSleep()).resolves.toBeUndefined();
    expect(fetchFn.mock.calls[2]?.[0]).toBe("http://vm:7878/api/sleep/stop");
  });

  it("manda o pick para a rota dos picks e pede o preview do prompt (F12.4)", async () => {
    const fetchFn = fakeFetch(200, {
      target: "sessao",
      prompt: "Apontei…",
      file: ".assistente/picks/2026-10-22-11-44-05.md",
    });
    const hub = new HubClient({ url: "", token: null }, fetchFn);
    const pick = {
      url: "http://localhost:5173",
      selector: "button.salvar",
      text: "Salvar",
      html: "",
      tag: "button",
    };
    await hub.sendPick("pessoal", "a b", {
      target: "sessao",
      session: "pessoal--x--claude-1",
      request: "deixa verde",
      pick,
    });
    expect(fetchFn.mock.calls[0]?.[0]).toBe("/api/projects/pessoal/a%20b/picks");
    expect(fetchFn.mock.calls[0]?.[1]).toMatchObject({ method: "POST" });

    await hub.previewPick("pessoal", "game", { pick, request: "deixa verde" });
    expect(fetchFn.mock.calls[1]?.[0]).toBe("/api/projects/pessoal/game/picks/preview");
  });

  it("monta a URL do terminal em ws:// com o token na query", () => {
    const hub = new HubClient({ url: "http://10.77.0.10:7878", token: "a b" });
    expect(hub.terminalUrl("pessoal--x--claude-1")).toBe(
      "ws://10.77.0.10:7878/api/sessions/pessoal--x--claude-1/terminal?token=a%20b",
    );
  });
});
