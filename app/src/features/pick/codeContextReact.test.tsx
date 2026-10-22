import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";
import "./codeContext.js";

/** Componente de verdade: o React pendura o fiber no botão e, em dev, o `_debugStack`
 * (React 19) ou o `_debugSource` (React 18 e anteriores) com o lugar do JSX. */
function Botao() {
  return (
    <button type="button" className="primary">
      Salvar
    </button>
  );
}

function Card() {
  return (
    <div>
      <Botao />
    </div>
  );
}

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

/** Renderiza e devolve o botão. */
function renderizar(): HTMLButtonElement {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(<Card />);
  });
  const botao = container.querySelector("button");
  if (!botao) throw new Error("o botão não foi renderizado");
  return botao;
}

/** Tira do fiber os campos que só existem em dev (como num build de produção). */
function semCamposDeDev(elemento: Element) {
  const chave = Object.keys(elemento).find((k) => k.startsWith("__reactFiber$"));
  if (!chave) throw new Error("o React não pendurou o fiber no elemento");
  // biome-ignore lint/suspicious/noExplicitAny: o fiber do React não tem tipo público
  let fiber = (elemento as any)[chave];
  while (fiber) {
    delete fiber._debugStack;
    delete fiber._debugSource;
    fiber = fiber.return;
  }
}

describe("codeContext com React de verdade", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("acha o componente e o arquivo:linha do JSX em dev", () => {
    const botao = renderizar();
    const code = globalThis.__assistenteCodeContext?.extract(botao);

    expect(code?.react).toBe(true);
    expect(code?.component).toBe("Botao");
    expect(code?.stack).toEqual(["Botao", "Card"]);
    // Em dev o React expõe o lugar do JSX: o arquivo e a linha.
    expect(code?.source?.file).toContain("codeContextReact.test.tsx");
    expect(code?.source?.line).toBeGreaterThan(0);
  });

  it("em build de produção (sem os campos de dev) sobra o componente, sem arquivo", () => {
    const botao = renderizar();
    semCamposDeDev(botao);

    const code = globalThis.__assistenteCodeContext?.extract(botao);

    expect(code?.react).toBe(true);
    expect(code?.component).toBe("Botao");
    expect(code?.source).toBeNull();
  });
});
