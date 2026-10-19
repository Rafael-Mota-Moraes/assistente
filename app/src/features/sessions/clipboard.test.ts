import { describe, expect, it } from "vitest";
import { clearStaleTextarea, clipboardShortcut, dedupeComposedInput, osc52Text } from "./clipboard";

const key = (key: string, mods: KeyboardEventInit = {}) =>
  new KeyboardEvent("keydown", { key, ...mods });

describe("clipboardShortcut", () => {
  it("Ctrl+Shift+C copia e Ctrl+Shift+V cola", () => {
    expect(clipboardShortcut(key("C", { ctrlKey: true, shiftKey: true }))).toBe("copy");
    expect(clipboardShortcut(key("V", { ctrlKey: true, shiftKey: true }))).toBe("paste");
  });

  it("Ctrl+C e Ctrl+V sem Shift continuam indo para o agente", () => {
    expect(clipboardShortcut(key("c", { ctrlKey: true }))).toBeNull();
    expect(clipboardShortcut(key("v", { ctrlKey: true }))).toBeNull();
    expect(clipboardShortcut(key("C", { shiftKey: true }))).toBeNull();
    expect(clipboardShortcut(key("C", { ctrlKey: true, shiftKey: true, altKey: true }))).toBeNull();
  });
});

describe("osc52Text", () => {
  const b64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));

  it("decodifica o texto copiado pelo tmux, com acentos", () => {
    expect(osc52Text(`c;${b64("joão")}`)).toBe("joão");
    expect(osc52Text(`;${b64("x")}`)).toBe("x");
  });

  it("ignora pedidos de leitura e conteúdo inválido", () => {
    expect(osc52Text("c;?")).toBeNull();
    expect(osc52Text("c;")).toBeNull();
    expect(osc52Text("semseparador")).toBeNull();
    expect(osc52Text("c;%%%")).toBeNull();
  });
});

describe("dedupeComposedInput", () => {
  it("descarta o acento repetido logo em seguida", () => {
    let t = 0;
    const accept = dedupeComposedInput(50, () => t);
    expect(accept("ã")).toBe(true);
    t = 2;
    expect(accept("ã")).toBe(false);
    t = 400;
    expect(accept("ã")).toBe(true);
  });

  it("não mexe em ASCII nem em textos diferentes", () => {
    const accept = dedupeComposedInput(50, () => 0);
    expect(accept("a")).toBe(true);
    expect(accept("a")).toBe(true);
    expect(accept("é")).toBe(true);
    expect(accept("ã")).toBe(true);
  });
});

describe("clearStaleTextarea", () => {
  const setup = () => {
    const host = document.createElement("div");
    const textarea = document.createElement("textarea");
    host.append(textarea);
    clearStaleTextarea(host, textarea);
    const press = (init: KeyboardEventInit = {}) =>
      textarea.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init }));
    return { textarea, press };
  };

  it("esvazia a textarea a cada tecla fora de composição", () => {
    const { textarea, press } = setup();
    textarea.value = "a á ã";
    press({ key: "b" });
    expect(textarea.value).toBe("");
  });

  it("preserva o acento até o xterm.js ler depois do compositionend", async () => {
    const { textarea, press } = setup();
    textarea.dispatchEvent(new CompositionEvent("compositionstart"));
    textarea.value = "á";
    press({ key: "a", isComposing: true });
    textarea.dispatchEvent(new CompositionEvent("compositionend"));
    press({ key: " " });
    expect(textarea.value).toBe("á");
    await new Promise((resolve) => setTimeout(resolve, 0));
    press({ key: " " });
    expect(textarea.value).toBe("");
  });
});
