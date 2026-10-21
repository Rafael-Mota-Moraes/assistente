import { afterEach, describe, expect, it } from "vitest";
import {
  clearUiUpdateNotice,
  readUiUpdateNotice,
  rememberUiUpdate,
  sameUiCommit,
  shortUiHash,
  uiChanged,
} from "./uiUpdate";

afterEach(() => sessionStorage.clear());

describe("uiUpdate", () => {
  it("compara hashes curtos e completos do mesmo commit", () => {
    expect(sameUiCommit("abc1234", "abc1234def5678")).toBe(true);
    expect(sameUiCommit("abc1234def5678", "abc1234")).toBe(true);
    expect(sameUiCommit("abc1234", "def5678")).toBe(false);
    expect(sameUiCommit(null, "abc1234")).toBe(false);
    expect(sameUiCommit("abc1234", null)).toBe(false);
  });

  it("só acusa mudança quando já havia uma UI em uso", () => {
    expect(uiChanged(null, "abc1234")).toBe(false);
    expect(uiChanged(undefined, "abc1234")).toBe(false);
    expect(uiChanged("abc1234", null)).toBe(false);
    expect(uiChanged("abc1234", "abc1234")).toBe(false);
    expect(uiChanged("abc1234", "abc1234def")).toBe(false);
    expect(uiChanged("abc1234", "def5678")).toBe(true);
  });

  it("guarda, lê sem apagar (StrictMode) e limpa o aviso", () => {
    rememberUiUpdate({ commit: "abc1234", shell: true });
    expect(readUiUpdateNotice()).toEqual({ commit: "abc1234", shell: true });
    expect(readUiUpdateNotice()).toEqual({ commit: "abc1234", shell: true });
    clearUiUpdateNotice();
    expect(readUiUpdateNotice()).toBeNull();
  });

  it("ignora aviso inválido", () => {
    sessionStorage.setItem("assistente.ui-update", "não é json");
    expect(readUiUpdateNotice()).toBeNull();
    sessionStorage.setItem("assistente.ui-update", JSON.stringify({ shell: true }));
    expect(readUiUpdateNotice()).toBeNull();
  });

  it("encurta o hash para o aviso", () => {
    expect(shortUiHash("abc1234")).toBe("abc1234");
    expect(shortUiHash("0123456789abcdef")).toBe("0123456789");
  });
});
