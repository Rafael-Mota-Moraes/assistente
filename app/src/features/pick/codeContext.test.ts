import { beforeAll, describe, expect, it } from "vitest";
// O script é injetado na página do preview como script clássico; aqui ele é importado do
// mesmo jeito (o módulo só deixa a API em globalThis).
import "./codeContext.js";

/** Fiber de mentira: o React põe essas chaves no DOM, mas o teste só precisa do formato. */
type FakeFiber = {
  type: unknown;
  elementType?: unknown;
  return?: FakeFiber | null;
  _debugSource?: { fileName: string; lineNumber?: number; columnNumber?: number } | null;
  memoizedProps?: Record<string, unknown> | null;
  pendingProps?: Record<string, unknown> | null;
};

function Botao(): null {
  return null;
}
function Card(): null {
  return null;
}
function App(): null {
  return null;
}

/** Elemento do DOM com o fiber do React pendurado (como o React faz na página). */
function elementoComFiber(fiber: FakeFiber, chave = "__reactFiber$abc"): HTMLButtonElement {
  const botao = document.createElement("button");
  Object.defineProperty(botao, chave, { value: fiber, enumerable: true, configurable: true });
  return botao;
}

function api() {
  const loaded = globalThis.__assistenteCodeContext;
  if (!loaded) throw new Error("o script não deixou __assistenteCodeContext");
  return loaded;
}

describe("codeContext (script da página)", () => {
  beforeAll(() => {
    expect(globalThis.__assistenteCodeContext).toBeDefined();
  });

  it("sobe pelo fiber e traz componente, pilha e arquivo:linha", () => {
    const botao = elementoComFiber({
      type: "button",
      _debugSource: {
        fileName: "/src/features/x/Botao.tsx",
        lineNumber: 42,
        columnNumber: 7,
      },
      return: { type: Botao, return: { type: Card, return: { type: App, return: null } } },
    });

    const code = api().extract(botao);

    expect(code.react).toBe(true);
    expect(code.component).toBe("Botao");
    expect(code.stack).toEqual(["Botao", "Card", "App"]);
    expect(code.source).toEqual({
      file: "/src/features/x/Botao.tsx",
      line: 42,
      column: 7,
    });
  });

  it("sem _debugSource (build de produção) mantém o componente e não inventa arquivo", () => {
    const botao = elementoComFiber({
      type: "button",
      return: { type: Botao, return: { type: App, return: null } },
    });

    const code = api().extract(botao);

    expect(code.react).toBe(true);
    expect(code.component).toBe("Botao");
    expect(code.stack).toEqual(["Botao", "App"]);
    expect(code.source).toBeNull();
  });

  it("página sem React não inventa componente nem arquivo", () => {
    const botao = document.createElement("button");
    botao.textContent = "Salvar";

    const code = api().extract(botao);

    expect(code).toEqual({ react: false, component: null, stack: [], source: null });
  });

  it("aceita a chave antiga do React (__reactInternalInstance$)", () => {
    const botao = elementoComFiber(
      {
        type: "button",
        _debugSource: { fileName: "/src/features/x/Botao.tsx", lineNumber: 42 },
        return: { type: Botao, return: null },
      },
      "__reactInternalInstance$xyz",
    );

    const code = api().extract(botao);

    expect(code.component).toBe("Botao");
    expect(code.source?.file).toBe("/src/features/x/Botao.tsx");
    expect(code.source?.line).toBe(42);
  });

  it("pega o __source das props (React 17 e anteriores)", () => {
    const botao = elementoComFiber({
      type: "button",
      memoizedProps: { __source: { fileName: "/src/features/x/Botao.tsx", lineNumber: 42 } },
      return: { type: Botao, return: null },
    });

    const code = api().extract(botao);

    expect(code.source).toEqual({
      file: "/src/features/x/Botao.tsx",
      line: 42,
      column: null,
    });
  });

  it("elemento sem fiber nenhum não quebra", () => {
    expect(api().extract(null).react).toBe(false);
    expect(api().extract(undefined).react).toBe(false);
  });
});
