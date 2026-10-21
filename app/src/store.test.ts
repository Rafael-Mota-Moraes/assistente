import { beforeEach, describe, expect, it } from "vitest";
import { initialState, useAppStore } from "./store";

const KEY = "assistente-ui";

const saved = () =>
  JSON.parse(sessionStorage.getItem(KEY) ?? "{}") as { state?: Record<string, unknown> };

describe("estado persistido (F8.2)", () => {
  beforeEach(() => {
    sessionStorage.clear();
    useAppStore.setState(initialState, true);
  });

  it("guarda o lugar na navegação: perfil, projeto e aba ativa", () => {
    useAppStore.setState({
      activeProfile: "pessoal",
      activeProject: "game",
      activeSession: "pessoal--game--claude-2",
      closedTabs: ["pessoal--game--opencode-1"],
      lastAgent: "opencode",
    });
    expect(saved().state).toEqual({
      activeProfile: "pessoal",
      activeProject: "game",
      activeSession: "pessoal--game--claude-2",
      closedTabs: ["pessoal--game--opencode-1"],
      lastAgent: "opencode",
      views: [],
      activeView: null,
    });
  });

  it("não guarda conexão nem dados vivos (voltam do hubd)", () => {
    useAppStore.setState({
      activeProfile: "pessoal",
      health: {
        ok: true,
        version: "0.1.0",
        uptime_seconds: 1,
        commit: { hubd: "abc1234", ui: null },
        self_update: null,
      },
      toast: "algum aviso",
    });
    const state = saved().state ?? {};
    expect(state.hub).toBeUndefined();
    expect(state.health).toBeUndefined();
    expect(state.sessions).toBeUndefined();
    expect(state.toast).toBeUndefined();
  });

  it("recarregar a página volta para o mesmo lugar", () => {
    // Como um reload: o estado novo está no sessionStorage antes do store hidratar.
    sessionStorage.setItem(
      KEY,
      JSON.stringify({
        state: {
          activeProfile: "trabalho",
          activeProject: "erp",
          activeSession: "trabalho--erp--claude-1",
          closedTabs: [],
          lastAgent: "claude",
        },
        version: 0,
      }),
    );
    useAppStore.persist.rehydrate();
    const state = useAppStore.getState();
    expect(state.activeProfile).toBe("trabalho");
    expect(state.activeProject).toBe("erp");
    expect(state.activeSession).toBe("trabalho--erp--claude-1");
  });

  it("storage vazio ou corrompido não impede o estado inicial", () => {
    sessionStorage.setItem(KEY, "{ inválido");
    expect(() => useAppStore.persist.rehydrate()).not.toThrow();
    useAppStore.persist.rehydrate();
    expect(useAppStore.getState().activeProfile).toBeNull();
  });
});
