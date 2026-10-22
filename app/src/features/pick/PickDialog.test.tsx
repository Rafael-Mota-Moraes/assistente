import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HubClient } from "../../bridge/hub";
import type { PickDetail } from "../../bridge/types";
import { useAppStore } from "../../store";
import { PickDialog } from "./PickDialog";

const pick: PickDetail = {
  url: "http://localhost:5173",
  selector: "button.salvar",
  text: "Salvar",
  html: '<button class="salvar">Salvar</button>',
  tag: "button",
  source: "src/features/x/Botao.tsx:42",
};

const sessions = [
  {
    id: "pessoal--game--claude-1",
    profile: "pessoal",
    project: "game",
    agent: "claude" as const,
    created_at: 0,
    attached: 1,
    dead: false,
  },
  {
    id: "pessoal--game--opencode-1",
    profile: "pessoal",
    project: "game",
    agent: "opencode" as const,
    created_at: 0,
    attached: 0,
    dead: false,
  },
  {
    id: "pessoal--game--shell-1",
    profile: "pessoal",
    project: "game",
    agent: "shell" as const,
    created_at: 0,
    attached: 0,
    dead: false,
  },
];

function open() {
  const hub = {
    previewPick: vi.fn().mockResolvedValue({
      prompt:
        "Apontei na página http://localhost:5173 o elemento button.salvar. Pedido: deixa verde.",
    }),
    sendPick: vi.fn().mockResolvedValue({
      target: "sessao",
      session: "pessoal--game--claude-1",
      prompt:
        "Apontei na página http://localhost:5173 o elemento button.salvar. Pedido: deixa verde.",
      file: ".assistente/picks/2026-10-22-11-44-05.md",
    }),
  };
  useAppStore.setState({
    hub: hub as unknown as HubClient,
    pick: { profile: "pessoal", project: "game", pick },
    sessions,
    activeSession: "pessoal--game--claude-1",
  });
  render(<PickDialog />);
  return hub;
}

describe("PickDialog", () => {
  it("mostra o prompt e manda o pedido para a sessão escolhida", async () => {
    const hub = open();

    // O prompt que o hubd monta aparece no modal.
    expect(
      await screen.findByText(/Apontei na página http:\/\/localhost:5173/),
    ).toBeInTheDocument();
    // A aba Terminal não pode receber o pedido.
    expect(screen.queryByRole("option", { name: "pessoal--game--shell-1" })).toBeNull();

    fireEvent.change(screen.getByLabelText("O que quer mudar"), {
      target: { value: "deixa verde" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Enviar" }));

    await waitFor(() =>
      expect(hub.sendPick).toHaveBeenCalledWith("pessoal", "game", {
        target: "sessao",
        session: "pessoal--game--claude-1",
        request: "deixa verde",
        pick,
      }),
    );
    // O modal fecha, a aba vai para a frente e o aviso manda confirmar no terminal.
    await waitFor(() => expect(useAppStore.getState().pick).toBeNull());
    expect(useAppStore.getState().activeSession).toBe("pessoal--game--claude-1");
    expect(useAppStore.getState().toast).toMatch(/confirme no terminal/i);
  });

  it("troca de alvo: o pedido vira card no quadro", async () => {
    const hub = open();
    hub.sendPick.mockResolvedValue({
      target: "quadro",
      card: {
        id: "T7",
        title: "deixa verde",
        description: "",
        priority: "normal",
        kind: "manual",
        column: "backlog",
        created_at: 0,
      },
      prompt: "Apontei…",
      file: ".assistente/picks/2026-10-22-11-44-05.md",
    });

    await screen.findByText(/Apontei na página/);
    fireEvent.click(screen.getByRole("radio", { name: /Quadro/ }));
    // Com o quadro, não há sessão para escolher.
    expect(screen.queryByLabelText("Sessão que recebe o pedido")).toBeNull();

    fireEvent.change(screen.getByLabelText("O que quer mudar"), {
      target: { value: "deixa verde" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Enviar" }));

    await waitFor(() =>
      expect(hub.sendPick).toHaveBeenCalledWith("pessoal", "game", {
        target: "quadro",
        session: null,
        request: "deixa verde",
        pick,
      }),
    );
    await waitFor(() => expect(useAppStore.getState().pick).toBeNull());
    expect(useAppStore.getState().toast).toMatch(/T7/);
  });

  it("erro ao enviar fica no modal", async () => {
    const hub = open();
    hub.sendPick.mockRejectedValueOnce(new Error("a sessão escolhida é de outro projeto"));

    await screen.findByText(/Apontei na página/);
    fireEvent.change(screen.getByLabelText("O que quer mudar"), {
      target: { value: "deixa verde" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Enviar" }));

    expect(await screen.findByText("a sessão escolhida é de outro projeto")).toBeInTheDocument();
    // O modal continua aberto, com o texto que o usuário escreveu.
    expect(useAppStore.getState().pick).not.toBeNull();
    expect(screen.getByLabelText("O que quer mudar")).toHaveValue("deixa verde");
  });

  it("sem texto não há o que enviar", async () => {
    const hub = open();
    await screen.findByText(/Apontei na página/);
    expect(screen.getByRole("button", { name: "Enviar" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("O que quer mudar"), {
      target: { value: "   " },
    });
    expect(screen.getByRole("button", { name: "Enviar" })).toBeDisabled();
    expect(hub.sendPick).not.toHaveBeenCalled();
  });
});
