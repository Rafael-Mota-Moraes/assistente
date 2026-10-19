import { describe, expect, it } from "vitest";
import type { ProfileView, SessionInfo } from "../../bridge/types";
import { useAppStore } from "../../store";
import {
  closeTab,
  cycleProfile,
  cycleTab,
  reopenTab,
  selectTab,
  shortcutAction,
} from "./shortcuts";

const key = (key: string, mods: KeyboardEventInit = {}) =>
  new KeyboardEvent("keydown", { key, ...mods });

const ctrl = { ctrlKey: true };
const ctrlShift = { ctrlKey: true, shiftKey: true };

describe("shortcutAction", () => {
  it("Ctrl+Shift+T/W/P e Ctrl+Tab", () => {
    expect(shortcutAction(key("T", ctrlShift))).toEqual({ type: "new-session" });
    expect(shortcutAction(key("W", ctrlShift))).toEqual({ type: "close-tab" });
    expect(shortcutAction(key("P", ctrlShift))).toEqual({ type: "switch-profile" });
    expect(shortcutAction(key("Tab", ctrl))).toEqual({ type: "next-tab" });
  });

  it("Ctrl+1..9 seleciona a aba (0-based); Ctrl+0 não é atalho", () => {
    expect(shortcutAction(key("1", ctrl))).toEqual({ type: "select-tab", index: 0 });
    expect(shortcutAction(key("9", ctrl))).toEqual({ type: "select-tab", index: 8 });
    expect(shortcutAction(key("0", ctrl))).toBeNull();
  });

  it("sem Ctrl, com Alt/Meta ou outras teclas: não é atalho", () => {
    expect(shortcutAction(key("T", { shiftKey: true }))).toBeNull();
    expect(shortcutAction(key("T", ctrl))).toBeNull();
    expect(shortcutAction(key("T", { ctrlKey: true, altKey: true, shiftKey: true }))).toBeNull();
    expect(shortcutAction(key("T", { metaKey: true, shiftKey: true }))).toBeNull();
    expect(shortcutAction(key("x", ctrl))).toBeNull();
    expect(shortcutAction(key("Tab", ctrlShift))).toBeNull();
  });
});

const session = (id: string, project = "game"): SessionInfo => ({
  id,
  profile: "pessoal",
  project,
  agent: "claude",
  created_at: 0,
  attached: 0,
});

const profiles: ProfileView[] = [
  { id: "pessoal", label: "Pessoal", color: "#8b5cf6", root: "/p" },
  { id: "trabalho", label: "Trabalho", color: "#0ea5e9", root: "/t" },
];

function withTabs() {
  useAppStore.setState({
    activeProfile: "pessoal",
    activeProject: "game",
    profiles,
    sessions: [session("s1"), session("s2"), session("s3")],
    activeSession: "s1",
  });
}

describe("ações dos atalhos", () => {
  it("closeTab esconde a aba atual sem matar a sessão e foca a vizinha", () => {
    withTabs();
    useAppStore.setState({ activeSession: "s2" });
    closeTab();
    const state = useAppStore.getState();
    expect(state.closedTabs).toEqual(["s2"]);
    // As três sessões continuam vivas; só a aba saiu da barra.
    expect(state.sessions.map((s) => s.id)).toEqual(["s1", "s2", "s3"]);
    expect(state.activeSession).toBe("s3");

    // Fechando a última, o foco volta para a anterior.
    closeTab();
    expect(useAppStore.getState().activeSession).toBe("s1");
  });

  it("closeTab da única aba deixa activeSession nulo", () => {
    useAppStore.setState({
      activeProfile: "pessoal",
      activeProject: "game",
      sessions: [session("s1")],
      activeSession: "s1",
    });
    closeTab();
    expect(useAppStore.getState().closedTabs).toEqual(["s1"]);
    expect(useAppStore.getState().activeSession).toBeNull();
  });

  it("reopenTab devolve a aba e foca nela", () => {
    withTabs();
    closeTab();
    reopenTab("s1");
    const state = useAppStore.getState();
    expect(state.closedTabs).toEqual([]);
    expect(state.activeSession).toBe("s1");
  });

  it("cycleTab roda as abas visíveis em ciclo, pulando as fechadas", () => {
    withTabs();
    useAppStore.setState({ closedTabs: ["s2"] });
    cycleTab();
    expect(useAppStore.getState().activeSession).toBe("s3");
    cycleTab();
    expect(useAppStore.getState().activeSession).toBe("s1");
  });

  it("cycleTab com uma aba só não faz nada", () => {
    useAppStore.setState({
      activeProfile: "pessoal",
      activeProject: "game",
      sessions: [session("s1")],
      activeSession: "s1",
    });
    cycleTab();
    expect(useAppStore.getState().activeSession).toBe("s1");
  });

  it("selectTab foca a aba N e ignora índice fora da barra", () => {
    withTabs();
    selectTab(2);
    expect(useAppStore.getState().activeSession).toBe("s3");
    selectTab(9);
    expect(useAppStore.getState().activeSession).toBe("s3");
  });

  it("cycleProfile troca de perfil e limpa o projeto, como clicar no seletor", () => {
    useAppStore.setState({ profiles, activeProfile: "pessoal", activeProject: "game" });
    cycleProfile();
    const state = useAppStore.getState();
    expect(state.activeProfile).toBe("trabalho");
    expect(state.activeProject).toBeNull();
    cycleProfile();
    expect(useAppStore.getState().activeProfile).toBe("pessoal");
  });
});
