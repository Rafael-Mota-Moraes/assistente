import type { AgentKind } from "../../bridge/types";
import { useAppStore } from "../../store";

function fail(error: unknown) {
  useAppStore.setState({ actionError: error instanceof Error ? error.message : String(error) });
}

/** Abre uma sessão nova do agente no projeto ativo e já foca a aba dela. */
export async function openSession(agent: AgentKind) {
  const { hub, activeProfile, activeProject, sessions } = useAppStore.getState();
  if (!hub || !activeProfile || !activeProject) return;
  try {
    const session = await hub.createSession({
      profile: activeProfile,
      project: activeProject,
      agent,
    });
    useAppStore.setState({
      sessions: [...sessions.filter((s) => s.id !== session.id), session],
      activeSession: session.id,
      actionError: null,
    });
  } catch (error) {
    fail(error);
  }
}

/** Encerra de vez (mata a sessão no tmux), não só fecha a aba. */
export async function endSession(id: string) {
  const { hub } = useAppStore.getState();
  if (!hub) return;
  try {
    await hub.deleteSession(id);
    useAppStore.setState((s) => ({
      sessions: s.sessions.filter((x) => x.id !== id),
      activeSession: s.activeSession === id ? null : s.activeSession,
      actionError: null,
    }));
  } catch (error) {
    fail(error);
  }
}
