import { describe, expect, it } from "vitest";
import type { PickedElement } from "../../bridge/types";
import { toPickDetail } from "./detail";

const pick: PickedElement = {
  tag: "button",
  id: "salvar",
  classes: ["btn", "primary"],
  role: null,
  aria: { label: "Salvar" },
  text: "Salvar",
  html: '<button id="salvar">Salvar</button>',
  selector: "#salvar",
  dom_path: "body > main > button#salvar",
  rect: { x: 10, y: 20, width: 120, height: 32 },
  viewport: { width: 1280, height: 720 },
  scroll: { x: 0, y: 140 },
  url: "http://localhost:5173/",
};

describe("toPickDetail", () => {
  it("leva o elemento apontado para o formato do hubd", () => {
    expect(toPickDetail(pick)).toMatchObject({
      url: "http://localhost:5173/",
      selector: "#salvar",
      text: "Salvar",
      html: '<button id="salvar">Salvar</button>',
      tag: "button",
      id: "salvar",
      classes: ["btn", "primary"],
      aria_label: "Salvar",
      dom_path: "body > main > button#salvar",
      rect: { x: 10, y: 20, width: 120, height: 32 },
    });
  });

  it("texto e HTML ausentes viram texto vazio, sem aria-label", () => {
    const detail = toPickDetail({ ...pick, text: null, html: null, aria: {} });
    expect(detail.text).toBe("");
    expect(detail.html).toBe("");
    expect(detail.aria_label).toBeNull();
  });
});
