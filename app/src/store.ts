import { create } from "zustand";
import type { HubClient } from "./bridge/hub";
import type {
  Health,
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
  profiles: ProfileView[];
  activeProfile: string | null;
  projects: Project[];
  activeProject: string | null;
  /** Todas as sessões do hubd (de todos os perfis); a UI filtra pelo projeto ativo. */
  sessions: SessionInfo[];
  activeSession: string | null;
  /** Último erro de ação do usuário (criar/encerrar sessão), mostrado num aviso. */
  actionError: string | null;
};

export const initialState: State = {
  hub: null,
  hubError: null,
  health: null,
  vm: null,
  notebook: null,
  profiles: [],
  activeProfile: null,
  projects: [],
  activeProject: null,
  sessions: [],
  activeSession: null,
  actionError: null,
};

export const useAppStore = create<State>(() => initialState);
