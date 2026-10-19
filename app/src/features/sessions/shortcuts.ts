// Atalhos de teclado da área de sessões (spec 03, F2.4). `shortcutAction` é pura para o
// vitest; quem executa é o `useShortcuts` (listener no window) — o TerminalView só engole
// essas teclas para não irem ao agente.
import { useEffect } from "react";
import { useAppStore, visibleProjectSessions } from "../../store";
import { openSession } from "./actions";
import { clipboardShortcut } from "./clipboard";

export type ShortcutAction =
  | { type: "new-session" }
  | { type: "close-tab" }
  | { type: "next-tab" }
  | { type: "switch-profile" }
  | { type: "select-tab"; index: number };

type KeyLike = Pick<KeyboardEvent, "key" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

/** Ctrl+Shift+T/W/P, Ctrl+Tab, Ctrl+1..9. Qualquer outro modificador: não é atalho. */
export function shortcutAction(event: KeyLike): ShortcutAction | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  if (event.shiftKey) {
    switch (event.key) {
      case "T":
        return { type: "new-session" };
      case "W":
        return { type: "close-tab" };
      case "P":
        return { type: "switch-profile" };
      default:
        return null;
    }
  }
  if (event.key === "Tab") return { type: "next-tab" };
  if (/^[1-9]$/.test(event.key)) return { type: "select-tab", index: Number(event.key) - 1 };
  return null;
}

/** Fecha a aba atual: a sessão continua viva na VM, só sai da barra de abas.
 * O foco vai para a aba vizinha (a seguinte, ou a anterior se era a última). */
export function closeTab() {
  const state = useAppStore.getState();
  const visible = visibleProjectSessions(state);
  const index = state.activeSession ? visible.findIndex((s) => s.id === state.activeSession) : -1;
  const current = visible[index] ?? visible.at(-1);
  if (!current) return;
  const at = index === -1 ? visible.length - 1 : index;
  const neighbor = visible[at + 1] ?? visible[at - 1] ?? null;
  useAppStore.setState({
    closedTabs: [...state.closedTabs, current.id],
    activeSession: neighbor?.id ?? null,
  });
}

/** Devolve uma aba fechada à barra e foca nela. */
export function reopenTab(id: string) {
  useAppStore.setState((s) => ({
    closedTabs: s.closedTabs.filter((x) => x !== id),
    activeSession: id,
  }));
}

/** Ctrl+Tab: próxima aba visível, em ciclo. */
export function cycleTab() {
  const state = useAppStore.getState();
  const visible = visibleProjectSessions(state);
  if (visible.length < 2) return;
  const index = visible.findIndex((s) => s.id === state.activeSession);
  const next = visible[(index + 1) % visible.length];
  useAppStore.setState({ activeSession: next.id });
}

/** Ctrl+1..9: foca a aba N (1-based); se não existe, não faz nada. */
export function selectTab(index: number) {
  const visible = visibleProjectSessions(useAppStore.getState());
  const target = visible[index];
  if (target) useAppStore.setState({ activeSession: target.id });
}

/** Ctrl+Shift+P: próximo perfil, como clicar no seletor (limpa projeto e lista). */
export function cycleProfile() {
  const { profiles, activeProfile } = useAppStore.getState();
  if (profiles.length < 2) return;
  const index = profiles.findIndex((p) => p.id === activeProfile);
  const next = profiles[(index + 1) % profiles.length];
  useAppStore.setState({ activeProfile: next.id, activeProject: null, projects: [] });
}

export function runShortcut(action: ShortcutAction) {
  switch (action.type) {
    case "new-session":
      openSession(useAppStore.getState().lastAgent ?? "claude");
      break;
    case "close-tab":
      closeTab();
      break;
    case "next-tab":
      cycleTab();
      break;
    case "select-tab":
      selectTab(action.index);
      break;
    case "switch-profile":
      cycleProfile();
      break;
  }
}

/** Listener global: vale com o foco no terminal ou fora dele. */
export function useShortcuts() {
  useEffect(() => {
    function onKeydown(event: KeyboardEvent) {
      // Copiar/colar (Ctrl+Shift+C/V) o terminal resolve; não atropela.
      if (clipboardShortcut(event)) return;
      const action = shortcutAction(event);
      if (!action) return;
      event.preventDefault();
      runShortcut(action);
    }
    window.addEventListener("keydown", onKeydown);
    return () => window.removeEventListener("keydown", onKeydown);
  }, []);
}
