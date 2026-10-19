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
});
