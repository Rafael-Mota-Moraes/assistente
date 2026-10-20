import type { AgentKind } from "../../bridge/types";
import { useAppStore } from "../../store";

function fail(error: unknown) {
  useAppStore.setState({ actionError: error instanceof Error ? error.message : String(error) });
}

/** Comando do PC que faz o login do Claude no perfil (o login é por perfil). */
export function claudeLoginCommand(profile: string): string {
  return `bin/assistente login claude ${profile}`;
}

/** Abre uma sessão nova do agente no projeto ativo e já foca a aba dela. O Terminal não vira
 * `lastAgent`: o Ctrl+Shift+T continua abrindo o último agente. */
export async function openSession(agent: AgentKind) {
  const { hub, activeProfile, activeProject, sessions, profiles } = useAppStore.getState();
  if (!hub || !activeProfile || !activeProject) return;
  // Sem login o Claude abre e morre na hora dentro do jail: a aba sumiria sem explicação.
  const profile = profiles.find((p) => p.id === activeProfile);
  if (agent === "claude" && profile?.claude_login === false) {
    fail(
      `O Claude ainda não tem login no perfil ${profile.label}. No PC, rode: ` +
        `${claudeLoginCommand(profile.id)} — depois clique em Claude de novo.`,
    );
    return;
  }
  try {
    const session = await hub.createSession({
      profile: activeProfile,
      project: activeProject,
      agent,
    });
    useAppStore.setState({
      sessions: [...sessions.filter((s) => s.id !== session.id), session],
      activeSession: session.id,
      ...(agent === "shell" ? {} : { lastAgent: agent }),
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
      // Se a aba estava fechada, o id sai da lista: uma sessão nova com o mesmo id
      // (pessoal--game--claude-1 recriada) não pode nascer escondida.
      closedTabs: s.closedTabs.filter((x) => x !== id),
      actionError: null,
    }));
  } catch (error) {
    fail(error);
  }
}
