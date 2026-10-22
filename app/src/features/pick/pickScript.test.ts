// Teste do script injetado na webview do preview (F12.2). O arquivo é o mesmo que o
// Rust embute (`app/src-tauri/src/pick.js`), então o teste cobre o script de verdade.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type PickPayload = {
  tag: string;
  id: string | null;
  classes: string[];
  role: string | null;
  aria: Record<string, string>;
  text: string | null;
  html: string | null;
  selector: string;
  dom_path: string;
  rect: { x: number; y: number; width: number; height: number };
  viewport: { width: number; height: number };
  scroll: { x: number; y: number };
  url: string;
  code: {
    react: boolean;
    component: string | null;
    stack: string[];
    source: { file: string; line: number | null; column: number | null } | null;
  } | null;
};

type PickApi = {
  enable: () => void;
  setEnabled: (on: boolean) => void;
  disable: () => void;
  isEnabled: () => boolean;
  remove: () => void;
  last: PickPayload | null;
  navigate: (url: string) => void;
};

/** O script mora na casca (Rust); o teste da UI o procura a partir da raiz do projeto. */
function scriptPath(): string {
  let dir = process.cwd();
  for (let nivel = 0; nivel < 5; nivel += 1) {
    const candidate = join(dir, "src-tauri", "src", "pick.js");
    if (existsSync(candidate)) return candidate;
    dir = dirname(dir);
  }
  throw new Error("pick.js não encontrado");
}

const SCRIPT = readFileSync(scriptPath(), "utf8");

function api(): PickApi {
  return (window as unknown as { __assistentePick: PickApi }).__assistentePick;
}

/** Instala o script numa página com um botão que muda de cor quando clicado. */
function install() {
  document.body.innerHTML =
    '<main><button id="alvo" class="btn primary" aria-label="Salvar">Salvar</button></main>';
  const alvo = document.getElementById("alvo") as HTMLButtonElement;
  const acoes = vi.fn();
  alvo.addEventListener("click", () => {
    acoes();
    alvo.style.background = "green";
  });
  // O jsdom não tem elementFromPoint: o contorno segue o elemento devolvido aqui.
  document.elementFromPoint = () => alvo;
  new Function(SCRIPT)();
  // A navegação é trocada para o teste não navegar: o script continua o mesmo.
  api().navigate = vi.fn();
  return { alvo, acoes };
}

/** O app injeta o `__TAURI_INTERNALS__` com o rótulo da webview. */
function comRotulo(label: string | null) {
  if (label === null) {
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    return;
  }
  (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {
    metadata: { currentWebview: { label } },
  };
}

describe("script do modo apontar", () => {
  beforeEach(() => {
    delete (window as unknown as { __assistentePick?: PickApi }).__assistentePick;
    comRotulo("preview");
  });

  afterEach(() => {
    api()?.remove();
    delete (window as unknown as { __assistentePick?: PickApi }).__assistentePick;
    delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    document.body.innerHTML = "";
  });

  it("o botão flutuante liga o modo", () => {
    install();
    const botao = document.querySelector("[data-assistente-pick='button']") as HTMLButtonElement;
    expect(botao).not.toBeNull();
    expect(botao.textContent).toBe("Apontar");
    expect(api().isEnabled()).toBe(false);

    botao.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(api().isEnabled()).toBe(true);
    expect(botao.textContent).toMatch(/Apontando/);
  });

  it("o clique é capturado e a página não age", () => {
    const { alvo, acoes } = install();
    api().enable();

    alvo.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));

    // A ação da página não rodou: o clique foi tomado na fase de captura.
    expect(acoes).not.toHaveBeenCalled();
    expect(alvo.style.background).toBe("");

    // E o pick saiu pela navegação `assistente-pick:`.
    const navegar = api().navigate as unknown as ReturnType<typeof vi.fn>;
    expect(navegar).toHaveBeenCalledTimes(1);
    const url = navegar.mock.calls[0][0] as string;
    expect(url.startsWith("assistente-pick:")).toBe(true);

    const payload = JSON.parse(
      decodeURIComponent(url.slice("assistente-pick:".length)),
    ) as PickPayload;
    expect(payload.tag).toBe("button");
    expect(payload.id).toBe("alvo");
    expect(payload.classes).toEqual(["btn", "primary"]);
    expect(payload.aria.label).toBe("Salvar");
    expect(payload.text).toBe("Salvar");
    expect(payload.selector).toBe("#alvo");
    expect(payload.dom_path).toContain("button#alvo");
  });

  it("um novo clique troca a seleção", () => {
    const { alvo } = install();
    api().enable();
    alvo.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(api().last?.id).toBe("alvo");

    const outro = document.createElement("a");
    outro.id = "link";
    outro.textContent = "ir";
    document.querySelector("main")?.appendChild(outro);
    outro.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));

    expect(api().last?.id).toBe("link");
    expect(api().last?.tag).toBe("a");
  });

  it("o botão da barra do preview liga e desliga pelo setEnabled (F12.1)", () => {
    const { alvo, acoes } = install();
    api().setEnabled(true);
    expect(api().isEnabled()).toBe(true);
    alvo.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(acoes).not.toHaveBeenCalled();
    expect(api().last?.id).toBe("alvo");

    api().setEnabled(false);
    expect(api().isEnabled()).toBe(false);
    alvo.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(acoes).toHaveBeenCalledTimes(1);
  });

  it("Esc cancela o modo", () => {
    const { alvo } = install();
    api().enable();
    expect(api().isEnabled()).toBe(true);

    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(api().isEnabled()).toBe(false);

    // Cancelado, o clique passa direto para a página.
    alvo.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
    expect(api().last).toBeNull();
  });

  it("o elemento sob o mouse ganha contorno", async () => {
    const { alvo } = install();
    api().enable();
    alvo.getBoundingClientRect = () =>
      ({ x: 5, y: 6, width: 70, height: 20, top: 6, left: 5, right: 75, bottom: 26 }) as DOMRect;

    document.dispatchEvent(
      new MouseEvent("mousemove", { bubbles: true, clientX: 10, clientY: 10 }),
    );
    await new Promise((resolve) => setTimeout(resolve, 40));

    const contorno = document.querySelector("[data-assistente-pick='outline']") as HTMLElement;
    expect(contorno).not.toBeNull();
    expect(contorno.style.display).toBe("block");
    expect(contorno.style.left).toBe("5px");
    expect(contorno.style.width).toBe("70px");
  });

  it("a janela principal e o painel da memória não ganham o botão", () => {
    for (const label of ["main", "memoria"]) {
      comRotulo(label);
      new Function(SCRIPT)();
      expect(document.querySelector("[data-assistente-pick='button']")).toBeNull();
      expect(
        (window as unknown as { __assistentePick?: PickApi }).__assistentePick,
      ).toBeUndefined();
    }
  });

  it("sem o rótulo da webview o script não faz nada", () => {
    comRotulo(null);
    new Function(SCRIPT)();
    expect(document.querySelector("[data-assistente-pick='button']")).toBeNull();
    expect((window as unknown as { __assistentePick?: PickApi }).__assistentePick).toBeUndefined();
  });
});
