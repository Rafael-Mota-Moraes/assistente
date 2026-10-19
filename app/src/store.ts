import { create } from "zustand";
import type { HubClient } from "./bridge/hub";
import type { Health, NotebookStatus, ProfileView, Project, VmStatus } from "./bridge/types";

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
};

export const useAppStore = create<State>(() => initialState);
