import { describe, expect, it } from "vitest";
import type { PickCodeContext } from "../../bridge/types";
import { resolveCodeContext, toProjectPath } from "./context";

/** Dev: a página é Vite + React, então o `_debugSource` diz o arquivo e a linha. */
const codeDev: PickCodeContext = {
  react: true,
  component: "Botao",
  stack: ["Botao", "Card", "App"],
  source: { file: "/src/features/x/Botao.tsx", line: 42, column: 7 },
};

/** Produção: build sem `_debugSource`, sobra o componente. */
const codeProducao: PickCodeContext = { ...codeDev, source: null };

/** Página sem React: nem componente nem arquivo. */
const codeSemReact: PickCodeContext = { react: false, component: null, stack: [], source: null };

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
    const context = resolveCodeContext(codeDev);

    expect(context.source?.path).toBe("src/features/x/Botao.tsx");
    expect(context.source?.line).toBe(42);
    expect(context.source?.label).toBe("src/features/x/Botao.tsx:42");
    expect(context.component).toBe("Botao");
    expect(context.stack).toEqual(["Botao", "Card", "App"]);
  });

  it("sem fonte não inventa arquivo", () => {
    const context = resolveCodeContext(codeProducao);

    expect(context.source).toBeNull();
    expect(context.component).toBe("Botao");
  });

  it("aceita página sem React", () => {
    const context = resolveCodeContext(codeSemReact);

    expect(context.react).toBe(false);
    expect(context.component).toBeNull();
    expect(context.source).toBeNull();
  });

  it("usa a raiz do projeto quando a fonte vem com caminho absoluto", () => {
    const context = resolveCodeContext(
      { ...codeDev, source: { file: "/home/dev/proj/lib/App.tsx", line: null, column: null } },
      "/home/dev/proj",
    );

    expect(context.source?.label).toBe("lib/App.tsx");
  });
});
