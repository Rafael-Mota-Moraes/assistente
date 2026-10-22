import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PickedElement } from "../../bridge/types";
import { useAppStore } from "../../store";
import { PickDialog } from "./PickDialog";

// O Rust emite o evento `preview-pick`; a UI escuta pelo mesmo `listen` do Tauri.
const { ouvintes } = vi.hoisted(() => ({
  ouvintes: [] as ((event: { payload: PickedElement }) => void)[],
}));

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_evento: string, handler: (event: { payload: PickedElement }) => void) => {
    ouvintes.push(handler);
    return () => {};
  }),
}));

const pick: PickedElement = {
  tag: "button",
  id: "salvar",
  classes: ["btn", "primary"],
  role: null,
  aria: { label: "Salvar" },
  text: "Salvar",
  html: '<button id="salvar" class="btn primary">Salvar</button>',
  selector: "#salvar",
  dom_path: "body > main > button#salvar",
  rect: { x: 10, y: 20, width: 120, height: 32 },
  viewport: { width: 1280, height: 720 },
  scroll: { x: 0, y: 140 },
  url: "http://localhost:5173/",
};

function open(picked: PickedElement = pick) {
  useAppStore.setState({ pick: picked });
  render(<PickDialog />);
}

describe("PickDialog", () => {
  it("mostra o elemento apontado e o espaço do pedido", () => {
    open();
    expect(screen.getByRole("dialog", { name: "Apontar elemento" })).toBeInTheDocument();
    // O id aparece na linha do elemento e o seletor logo abaixo: "#salvar" duas vezes.
    expect(screen.getAllByText("#salvar")).toHaveLength(2);
    expect(screen.getByText("Salvar")).toBeInTheDocument();
    expect(screen.getByText("body > main > button#salvar")).toBeInTheDocument();
    expect(screen.getByText("label=Salvar")).toBeInTheDocument();
    expect(screen.getByText("120×32 em (10, 20)")).toBeInTheDocument();
    expect(screen.getByText("1280×720, rolagem (0, 140)")).toBeInTheDocument();
    expect(screen.getByText("http://localhost:5173/")).toBeInTheDocument();
    expect(
      screen.getByText('<button id="salvar" class="btn primary">Salvar</button>'),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("O que você quer que mude?")).toBeInTheDocument();
  });

  it("cancelar fecha a caixa de pedido", () => {
    open();
    fireEvent.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(useAppStore.getState().pick).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Apontar elemento" })).not.toBeInTheDocument();
  });

  it("um apontamento novo começa com o pedido em branco", () => {
    open();
    const campo = screen.getByLabelText("O que você quer que mude?");
    fireEvent.change(campo, { target: { value: "deixa verde" } });
    expect(campo).toHaveValue("deixa verde");

    act(() => useAppStore.setState({ pick: { ...pick, id: "outro" } }));
    expect(screen.getByLabelText("O que você quer que mude?")).toHaveValue("");
  });

  it("sem apontar não abre nada", () => {
    render(<PickDialog />);
    expect(screen.queryByRole("dialog", { name: "Apontar elemento" })).not.toBeInTheDocument();
  });

  it("o evento do Rust abre a caixa de pedido com o elemento", async () => {
    render(<PickDialog />);
    await waitFor(() => expect(ouvintes.length).toBeGreaterThan(0));

    act(() => ouvintes[ouvintes.length - 1]({ payload: pick }));

    expect(screen.getByRole("dialog", { name: "Apontar elemento" })).toBeInTheDocument();
    // O id aparece na linha do elemento e no seletor: "#salvar" duas vezes.
    expect(screen.getAllByText("#salvar")).toHaveLength(2);
    expect(screen.getByText("body > main > button#salvar")).toBeInTheDocument();
    expect(useAppStore.getState().pick).toEqual(pick);
  });
});
