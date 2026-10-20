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

  it("monta a URL do terminal em ws:// com o token na query", () => {
    const hub = new HubClient({ url: "http://10.77.0.10:7878", token: "a b" });
    expect(hub.terminalUrl("pessoal--x--claude-1")).toBe(
      "ws://10.77.0.10:7878/api/sessions/pessoal--x--claude-1/terminal?token=a%20b",
    );
  });
});
