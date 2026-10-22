import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PickDialog } from "./PickDialog";
import type { PickElement } from "./types";

/** Pick de dev: Vite + React, então o `_debugSource` diz o arquivo e a linha. */
const pickDev: PickElement = {
  url: "http://localhost:5173/",
  tag: "button",
  text: "Salvar",
  selector: "button.primary",
  classes: ["primary"],
  code: {
    react: true,
    component: "Botao",
    stack: ["Botao", "Card", "App"],
    source: { file: "/src/features/x/Botao.tsx", line: 42, column: 7 },
  },
};

/** Pick de produção: sem `_debugSource`, sobra o componente. */
const pickProducao: PickElement = {
  ...pickDev,
  code: { react: true, component: "Botao", stack: ["Botao", "Card", "App"], source: null },
};

/** Bloco "onde está" (sem o texto que vai para o agente, que é outro bloco). */
function onde() {
  return within(screen.getByRole("region", { name: "Onde está o elemento" }));
}

describe("PickDialog", () => {
  it("em dev mostra o arquivo:linha do elemento apontado", () => {
    render(<PickDialog pick={pickDev} onClose={() => {}} />);

    expect(onde().getByText("src/features/x/Botao.tsx:42")).toBeInTheDocument();
    expect(onde().getByText(/componente Botao/)).toBeInTheDocument();
    expect(onde().queryByText(/Sem arquivo de origem/)).not.toBeInTheDocument();
  });

  it("em produção mostra o componente e manda procurar o arquivo antes de editar", () => {
    render(<PickDialog pick={pickProducao} onClose={() => {}} />);

    expect(onde().getByText("Sem arquivo de origem")).toBeInTheDocument();
    expect(onde().getByText(/componente Botao/)).toBeInTheDocument();
    expect(onde().getByText(/grep por o texto "Salvar"/)).toBeInTheDocument();
    expect(onde().getByText(/não chute o arquivo/)).toBeInTheDocument();
  });

  it("o texto que vai para o agente leva o arquivo:linha", () => {
    render(<PickDialog pick={pickDev} onClose={() => {}} />);

    const preview = screen.getByText(/Apontei este elemento/);
    expect(preview).toHaveTextContent("src/features/x/Botao.tsx:42");
    expect(preview).toHaveTextContent("Pedido:");
  });

  it("envia o pedido e o texto montado", () => {
    const onSend = vi.fn();
    render(<PickDialog pick={pickDev} onClose={() => {}} onSend={onSend} />);

    const enviar = screen.getByRole("button", { name: "Enviar" });
    expect(enviar).toBeDisabled();

    fireEvent.change(screen.getByLabelText("O que você quer mudar?"), {
      target: { value: "deixa verde" },
    });
    fireEvent.click(enviar);

    expect(onSend).toHaveBeenCalledWith(
      "deixa verde",
      expect.stringContaining("src/features/x/Botao.tsx:42"),
    );
  });

  it("sem pick não mostra nada", () => {
    render(<PickDialog pick={null} onClose={() => {}} />);

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
