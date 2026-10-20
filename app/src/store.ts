import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import type { HubClient } from "./bridge/hub";
import type {
  AgentKind,
  CollectReport,
  Health,
  MemoryStatus,
  NotebookStatus,
  ProfileView,
  Project,
  SessionInfo,
  VmStatus,
} from "./bridge/types";

/** Estado do modal "Bom dia" (F6.6); `null` = fechado. */
export type MorningState = {
  profile: string;
  project: string;
  /** Noite do `dorme` (`YYYY-MM-DD`). */
  date: string;
  report: CollectReport | null;
  error: string | null;
  merged: boolean;
  busy: boolean;
};

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
  /** Modal "Bom dia" (F6.6): revisar e mesclar o que o agente fez no notebook. */
  morning: MorningState | null;
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
  morning: null,
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

/** F8.2: recarregar a página (ou o hubd aplicar uma UI nova, F8.7) volta para o mesmo
 * lugar. Só o lugar na navegação persiste; conexão e dados vivos vêm do hubd de novo. */
export const useAppStore = create<State>()(
  persist(() => initialState, {
    name: "assistente-ui",
    storage: createJSONStorage(() => sessionStorage),
    partialize: (state) => ({
      activeProfile: state.activeProfile,
      activeProject: state.activeProject,
      activeSession: state.activeSession,
      closedTabs: state.closedTabs,
      lastAgent: state.lastAgent,
    }),
  }),
);
