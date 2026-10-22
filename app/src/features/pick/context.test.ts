import { describe, expect, it } from "vitest";
import {
  describeComponents,
  describeWhere,
  resolveCodeContext,
  searchHints,
  searchInstruction,
  toProjectPath,
} from "./context";
import type { PickElement } from "./types";

/** Pick de dev: a página é Vite + React, então o `_debugSource` diz o arquivo e a linha. */
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

/** Pick de produção: build sem `_debugSource`, sobra o componente. */
const pickProducao: PickElement = {
  ...pickDev,
  code: { react: true, component: "Botao", stack: ["Botao", "Card", "App"], source: null },
};

/** Página sem React: nem componente nem arquivo. */
const pickSemReact: PickElement = {
  url: "http://exemplo.test/",
  tag: "button",
  text: "Salvar",
  selector: "button.primary",
  code: { react: false, component: null, stack: [], source: null },
};

describe("toProjectPath", () => {
  it("tira a barra do /src do dev server do Vite", () => {
    expect(toProjectPath("/src/features/x/Botao.tsx")).toBe("src/features/x/Botao.tsx");
  });

  it("tira a origem do dev server", () => {
    expect(toProjectPath("http://localhost:5173/src/features/x/Botao.tsx")).toBe(
      "src/features/x/Botao.tsx",
    );
  });

  it("tira a raiz do projeto de um caminho absoluto", () => {
    expect(toProjectPath("/home/dev/proj/src/features/x/Botao.tsx", "/home/dev/proj")).toBe(
      "src/features/x/Botao.tsx",
    );
  });

  it("deixa o caminho como está quando não dá para mapear", () => {
    expect(toProjectPath("/opt/app/index.html")).toBe("/opt/app/index.html");
  });
});

describe("resolveCodeContext", () => {
  it("acha o arquivo:linha no caminho do projeto", () => {
    const context = resolveCodeContext(pickDev.code);

    expect(context.source?.path).toBe("src/features/x/Botao.tsx");
    expect(context.source?.line).toBe(42);
    expect(context.source?.label).toBe("src/features/x/Botao.tsx:42");
    expect(context.component).toBe("Botao");
    expect(context.stack).toEqual(["Botao", "Card", "App"]);
  });

  it("sem fonte não inventa arquivo", () => {
    const context = resolveCodeContext(pickProducao.code);

    expect(context.source).toBeNull();
    expect(context.component).toBe("Botao");
  });

  it("aceita página sem React", () => {
    const context = resolveCodeContext(pickSemReact.code);

    expect(context.react).toBe(false);
    expect(context.component).toBeNull();
    expect(context.source).toBeNull();
  });
});

describe("describeWhere", () => {
  it("em dev mostra o arquivo:linha e o componente", () => {
    const where = describeWhere(resolveCodeContext(pickDev.code), pickDev);

    expect(where).toContain("src/features/x/Botao.tsx:42");
    expect(where).toContain("componente Botao");
    expect(where).toContain("pilha: Botao > Card > App");
  });

  it("sem fonte mostra componente, seletor e texto", () => {
    const where = describeWhere(resolveCodeContext(pickProducao.code), pickProducao);

    expect(where).toContain("componente Botao");
    expect(where).toContain("seletor button.primary");
    expect(where).toContain('texto "Salvar"');
    expect(where).not.toContain("Botao.tsx");
  });
});

describe("searchInstruction", () => {
  it("manda procurar pelo texto, pela classe e pelo componente", () => {
    const context = resolveCodeContext(pickProducao.code);
    const instruction = searchInstruction(pickProducao, context);

    expect(instruction).toContain('grep por o texto "Salvar"');
    expect(instruction).toContain('a classe "primary"');
    expect(instruction).toContain('o componente "Botao"');
    expect(instruction).toContain("não chute");
  });

  it("página sem React manda procurar pelo texto ou pelo seletor", () => {
    const context = resolveCodeContext(pickSemReact.code);

    expect(searchHints(pickSemReact, context)).toEqual(['o texto "Salvar"']);
    expect(searchInstruction(pickSemReact, context)).toContain('grep por o texto "Salvar"');

    const semTexto = { ...pickSemReact, text: "" };
    expect(searchHints(semTexto, context)).toEqual([]);
    expect(searchInstruction(semTexto, context)).toContain("grep pelo seletor");
  });

  it("em dev o pedido não precisa do aviso de procurar", () => {
    const context = resolveCodeContext(pickDev.code);

    expect(context.source).not.toBeNull();
    expect(describeComponents(context)).toBe("componente Botao · pilha: Botao > Card > App");
  });
});
