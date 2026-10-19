import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useAppStore } from "../../store";
import { StatusBar } from "./StatusBar";

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
          { name: "9router", port: 20128, up: true },
          { name: "ai_memory", port: 49374, up: false },
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
