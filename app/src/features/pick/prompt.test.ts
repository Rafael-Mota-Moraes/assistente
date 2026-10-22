import { describe, expect, it } from "vitest";
import { buildPickPrompt } from "./prompt";
import type { PickElement } from "./types";

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

const pickProducao: PickElement = {
  ...pickDev,
  code: { react: true, component: "Botao", stack: ["Botao", "Card", "App"], source: null },
};

describe("buildPickPrompt", () => {
  it("em dev manda o arquivo:linha do elemento", () => {
    const prompt = buildPickPrompt(pickDev, "deixa verde");

    expect(prompt).toContain("http://localhost:5173/");
    expect(prompt).toContain("src/features/x/Botao.tsx:42");
    expect(prompt).toContain("componente Botao");
    expect(prompt).toContain("Pedido: deixa verde");
    expect(prompt).not.toContain("Sem arquivo de origem");
  });

  it("em produção manda o componente e a dica de procurar o arquivo", () => {
    const prompt = buildPickPrompt(pickProducao, "deixa verde");

    expect(prompt).toContain("componente Botao");
    expect(prompt).toContain("seletor button.primary");
    expect(prompt).toContain('texto "Salvar"');
    expect(prompt).toContain("Sem arquivo de origem");
    expect(prompt).toContain("grep por o texto");
    expect(prompt).toContain("não chute");
    expect(prompt).toContain("Pedido: deixa verde");
  });
});
