import { create } from "zustand";
import type { HubClient } from "./bridge/hub";
import type {
  AgentKind,
  Health,
  MemoryStatus,
  NotebookStatus,
  ProfileView,
  Project,
  SessionInfo,
  VmStatus,
} from "./bridge/types";

type State = {
  hub: HubClient | null;
  /** Erro de conexão/config: a UI mostra no lugar do conteúdo. */
  hubError: string | null;
  health: Health | null;
  vm: VmStatus | null;
  notebook: NotebookStatus | null;
  memory: MemoryStatus | null;
  profiles: ProfileView[];
  activeProfile: string | null;
  projects: Project[];
  activeProject: string | null;
  /** Todas as sessões do hubd (de todos os perfis); a UI filtra pelo projeto ativo. */
  sessions: SessionInfo[];
  activeSession: string | null;
  /** Abas fechadas (Ctrl+Shift+W): a sessão continua viva na VM, só sai da barra de abas. */
  closedTabs: string[];
  /** Último agente usado em openSession; o Ctrl+Shift+T abre uma sessão nova dele. */
  lastAgent: AgentKind | null;
  /** Último erro de ação do usuário (criar/encerrar sessão), mostrado num aviso. */
  actionError: string | null;
  /** Mensagem temporária no rodapé (ex.: "não consegui abrir o Cursor"); some sozinha. */
  toast: string | null;
};

export const initialState: State = {
  hub: null,
  hubError: null,
  health: null,
  vm: null,
  notebook: null,
  memory: null,
  profiles: [],
  activeProfile: null,
  projects: [],
  activeProject: null,
  sessions: [],
  activeSession: null,
  closedTabs: [],
  lastAgent: null,
  actionError: null,
  toast: null,
};

/** Sessões visíveis (abas) do projeto ativo: as do perfil/projeto, menos as abas fechadas. */
export function visibleProjectSessions(
  state: Pick<State, "activeProfile" | "activeProject" | "sessions" | "closedTabs">,
): SessionInfo[] {
  if (!state.activeProject) return [];
  return state.sessions.filter(
    (s) =>
      s.profile === state.activeProfile &&
      s.project === state.activeProject &&
      !state.closedTabs.includes(s.id),
  );
}

export const useAppStore = create<State>(() => initialState);
