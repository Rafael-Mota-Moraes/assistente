import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import type { HubClient } from "./bridge/hub";
import type {
  AgentKind,
  CollectReport,
  ExecView,
  Health,
  MemoryStatus,
  NotebookStatus,
  PickDetail,
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

/** Modal de apontar e pedir (F12.4); `null` = fechado. */
export type PickState = {
  profile: string;
  project: string;
  /** O elemento apontado na página. */
  pick: PickDetail;
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
  /** Chat do Claude que monta o quadro (Popular pelo backlog): vive dentro do quadro, sem aba.
   * `key` = `perfil/projeto`, para o chat não aparecer em outro projeto. */
  boardChat: { key: string; sessionId: string } | null;
  /** Último agente usado em openSession; o Ctrl+Shift+T abre uma sessão nova dele. */
  lastAgent: AgentKind | null;
  /** Último erro de ação do usuário (criar/encerrar sessão), mostrado num aviso. */
  actionError: string | null;
  /** Mensagem temporária no rodapé (ex.: "não consegui abrir o Cursor"); some sozinha. */
  toast: string | null;
  /** Modal "Bom dia" (F6.6): revisar e mesclar o que o agente fez no notebook. */
  morning: MorningState | null;
  /** Modal de configurações do projeto (engrenagem, F9.3); `null` = fechado. */
  projectSettings: { profile: string; project: string } | null;
  /** Caixa de pedido do elemento apontado no preview (F12.2/F12.4); `null` = fechada. */
  pick: PickState | null;
  /** Visões abertas como abas (Quadro e Editor), por projeto: `key` = `perfil/projeto`. */
  views: { key: string; kind: ViewKind }[];
  /** Aba de visão em foco; `null` = uma sessão de agente. */
  activeView: { key: string; kind: ViewKind } | null;
  /** Painel da web UI do ai-memory (F5.5); `true` = aberto. */
  memoryPanelOpen: boolean;
  /** Painel do Assistente (F8) aberto como modal; `true` = aberto. */
  assistantPanelOpen: boolean;
  /** Estado do ai-memory (F5.6) aberto como modal; `true` = aberto. */
  memoryStatusOpen: boolean;
  /** Pedidos dos agentes para rodar fora do jail (F9.7), atualizados a cada 2 s. */
  execRequests: ExecView[];
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
  boardChat: null,
  lastAgent: null,
  actionError: null,
  toast: null,
  morning: null,
  projectSettings: null,
  pick: null,
  memoryPanelOpen: false,
  assistantPanelOpen: false,
  memoryStatusOpen: false,
  execRequests: [],
  views: [],
  activeView: null,
};

/** Visões que aparecem como abas: o Quadro, o Editor e o Preview de cada projeto. */
export type ViewKind = "board" | "editor" | "preview";

/** Chave de um projeto nas visões (`perfil/projeto`). */
export function viewKey(profile: string, project: string): string {
  return `${profile}/${project}`;
}

/** Foca uma sessão de agente: sai da visão em foco. */
export function selectSession(id: string | null) {
  useAppStore.setState({ activeSession: id, activeView: null });
}

/** Abre (ou só foca) a visão como aba do projeto. */
export function openView(key: string, kind: ViewKind) {
  useAppStore.setState((s) => ({
    views: s.views.some((v) => v.key === key && v.kind === kind)
      ? s.views
      : [...s.views, { key, kind }],
    activeView: { key, kind },
  }));
}

/** Abre a caixa de pedido do apontamento (F12.4). */
export function openPick(profile: string, project: string, pick: PickDetail) {
  useAppStore.setState({ pick: { profile, project, pick } });
}

/** Fecha a aba da visão; se ela estava em foco, volta para a sessão. */
export function closeView(key: string, kind: ViewKind) {
  useAppStore.setState((s) => ({
    views: s.views.filter((v) => !(v.key === key && v.kind === kind)),
    activeView: s.activeView?.key === key && s.activeView.kind === kind ? null : s.activeView,
  }));
}

/** Sessões visíveis (abas) do projeto ativo: as do perfil/projeto, menos as abas fechadas. */
export function visibleProjectSessions(
  state: Pick<State, "activeProfile" | "activeProject" | "sessions" | "closedTabs" | "boardChat">,
): SessionInfo[] {
  if (!state.activeProject) return [];
  return state.sessions.filter(
    (s) =>
      s.profile === state.activeProfile &&
      s.project === state.activeProject &&
      !state.closedTabs.includes(s.id) &&
      s.id !== state.boardChat?.sessionId,
  );
}

/** Modais do app que ficariam embaixo das webviews filhas (preview F12.1, memória F5.5),
 * que são widgets nativos por cima de toda a UI (F12.6): configurações do projeto, caixa do
 * apontar, "Bom dia", painel do Assistente e estado do ai-memory. */
export function anyModalOpen(
  state: Pick<
    State,
    "projectSettings" | "pick" | "morning" | "assistantPanelOpen" | "memoryStatusOpen"
  >,
): boolean {
  return (
    state.projectSettings !== null ||
    state.pick !== null ||
    state.morning !== null ||
    state.assistantPanelOpen ||
    state.memoryStatusOpen
  );
}

/** Qualquer sobreposição que precisa ficar por cima da webview do preview (F12.6): os modais
 * de `anyModalOpen`, o aviso (toast) e os pedidos de execução pendentes (F9.7/F9.10). */
export function anyOverlayOpen(
  state: Parameters<typeof anyModalOpen>[0] & Pick<State, "toast" | "execRequests">,
): boolean {
  return (
    anyModalOpen(state) ||
    state.toast !== null ||
    state.execRequests.some((r) => r.state === "pending")
  );
}

/** A webview do preview (F12.1) só existe com a aba "Preview" do projeto em foco: o que está
 * embaixo dela some. Serve para o app não desenhar por baixo da webview (F12.6). */
export function previewFocused(
  state: Pick<State, "activeView" | "activeProfile" | "activeProject">,
): boolean {
  if (state.activeView?.kind !== "preview") return false;
  if (state.activeProfile === null || state.activeProject === null) return false;
  return state.activeView.key === viewKey(state.activeProfile, state.activeProject);
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
      views: state.views,
      activeView: state.activeView,
    }),
  }),
);
