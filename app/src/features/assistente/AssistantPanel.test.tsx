import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HubClient } from "../../bridge/hub";
import type { Health, UpdateStatus } from "../../bridge/types";
import { useAppStore } from "../../store";
import { AssistantPanel } from "./AssistantPanel";

const status: UpdateStatus = {
  running: false,
  status: "idle",
  commit: null,
  current_ui: null,
  previous_commit: null,
  shell_changed: false,
  error: null,
  log_tail: null,
  log: null,
  head: "bbb2222",
  head_commit: "bbb2222",
  commits: [
    { hash: "bbb2222", subject: "feat: dois" },
    { hash: "aaa1111", subject: "feat: um" },
  ],
  pending_commits: [],
};

const health: Health = {
  ok: true,
  version: "0.1.0",
  uptime_seconds: 1,
  commit: { hubd: "zzz9999", ui: null },
  self_update: { profile: "pessoal", project: "assistente" },
};

function fakeHub(overrides: Partial<HubClient> = {}) {
  return {
    updateStatus: vi.fn(async () => status),
    applyToMain: vi.fn(async () => ({
      main: "ccc3333",
      applied: [{ hash: "aaa1111", subject: "feat: um" }],
    })),
    ...overrides,
  } as unknown as HubClient;
}

function open(hub: HubClient) {
  useAppStore.setState({ hub, health });
}

describe("AssistantPanel: juntar commits na main", () => {
  it("manda os commits marcados, na ordem em que foram marcados, e avisa o novo topo", async () => {
    const hub = fakeHub();
    open(hub);
    render(<AssistantPanel />);

    fireEvent.click(await screen.findByLabelText(/feat: dois/));
    fireEvent.click(screen.getByLabelText(/feat: um/));
    fireEvent.click(screen.getByRole("button", { name: "Aplicar 2 na main" }));

    expect(hub.applyToMain).toHaveBeenCalledWith(["bbb2222", "aaa1111"]);
    expect(await screen.findByText(/main agora em ccc3333: 1 commit/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Aplicar 0 na main" })).toBeDisabled();
  });

  it("sem nada marcado, o botão fica desligado e nada é enviado", async () => {
    const hub = fakeHub();
    open(hub);
    render(<AssistantPanel />);

    expect(await screen.findByRole("button", { name: "Aplicar 0 na main" })).toBeDisabled();
    expect(hub.applyToMain).not.toHaveBeenCalled();
  });

  it("erro do hubd (conflito) aparece na tela e mantém a seleção", async () => {
    const hub = fakeHub({
      applyToMain: vi.fn(async () => {
        throw new Error("não consegui aplicar aaa1111 na main. Nada mudou.");
      }),
    });
    open(hub);
    render(<AssistantPanel />);

    fireEvent.click(await screen.findByLabelText(/feat: um/));
    fireEvent.click(screen.getByRole("button", { name: "Aplicar 1 na main" }));

    expect(await screen.findByText(/Nada mudou/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Aplicar 1 na main" })).toBeEnabled();
  });
});
