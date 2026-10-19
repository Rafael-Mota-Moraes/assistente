import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useAppStore } from "../../store";
import { StatusBar, serviceTitle } from "./StatusBar";

describe("StatusBar", () => {
  it("mostra hubd offline quando não há saúde", () => {
    render(<StatusBar />);
    expect(screen.getByText("hubd offline")).toBeInTheDocument();
  });

  it("mostra VM, hubd e os serviços do notebook", () => {
    useAppStore.setState({
      vm: { state: "running", detail: null },
      health: { ok: true, version: "0.1.0", uptime_seconds: 3 },
      notebook: {
        ssh_host: "notebook",
        tunnel_managed: true,
        tunnel: { state: "up", last_error: null, restarts: 0, since: 0 },
        services: [
          { name: "9router", port: 20128, up: true, last_error: null, checked_at: 0 },
          {
            name: "ai_memory",
            port: 49374,
            up: false,
            last_error: "porta 49374 não escuta",
            checked_at: 0,
          },
        ],
      },
    });
    render(<StatusBar />);
    expect(screen.getByText("VM ligada")).toBeInTheDocument();
    expect(screen.getByText("hubd 0.1.0")).toBeInTheDocument();
    expect(screen.getByText("túnel").parentElement).toHaveAttribute("data-tone", "ok");
    expect(screen.getByText("9router").parentElement).toHaveAttribute("data-tone", "ok");
    expect(screen.getByText("ai-memory").parentElement).toHaveAttribute("data-tone", "off");
  });
});

describe("serviceTitle", () => {
  const service = { name: "9router", port: 20128, up: false, last_error: null, checked_at: 0 };

  it("caído mostra o erro do hubd", () => {
    expect(serviceTitle("notebook", { ...service, last_error: "HTTP 404 em /v1/models" })).toBe(
      "notebook → 127.0.0.1:20128 — HTTP 404 em /v1/models",
    );
  });

  it("caído sem erro conhecido diz 'sem resposta'", () => {
    expect(serviceTitle("notebook", service)).toContain("sem resposta");
  });

  it("mostra a hora da última verificação", () => {
    const title = serviceTitle("notebook", { ...service, up: true, checked_at: 1792425600 });
    expect(title).toMatch(/verificado às \d{2}:\d{2}:\d{2}/);
  });
});
