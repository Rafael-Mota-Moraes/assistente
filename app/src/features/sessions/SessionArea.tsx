import {
  Bot,
  Eye,
  FileCode2,
  LayoutGrid,
  RotateCcw,
  SquareChevronRight,
  TerminalSquare,
  X,
} from "lucide-react";
import type { ReactNode } from "react";
import type { AgentKind, SessionInfo } from "../../bridge/types";
import {
  closeView,
  openView,
  selectSession,
  useAppStore,
  type ViewKind,
  viewKey,
  visibleProjectSessions,
} from "../../store";
import { EditorPanel } from "../editor/EditorPanel";
import { KanbanBoard } from "../kanban/KanbanBoard";
import { PreviewPanel } from "../preview/PreviewPanel";
import { claudeLoginCommand, endSession, openSession } from "./actions";
import { reopenTab } from "./shortcuts";
import { TerminalView } from "./TerminalView";

const agents: { id: AgentKind; label: string; icon: typeof Bot; title: string }[] = [
  { id: "claude", label: "Claude", icon: Bot, title: "Nova sessão do Claude (no ai-jail da VM)" },
  {
    id: "opencode",
    label: "OpenCode",
    icon: TerminalSquare,
    title: "Nova sessão do OpenCode (no ai-jail da VM)",
  },
  {
    id: "shell",
    label: "Terminal",
    icon: SquareChevronRight,
    title: "Bash na pasta do projeto, na VM e fora do jail (Docker, rede, git push)",
  },
];

const agentLabel: Record<AgentKind, string> = {
  claude: "Claude",
  opencode: "OpenCode",
  shell: "Terminal",
};

/** Âmbar lembra que o Terminal roda fora do jail. */
const outsideJail = "#f59e0b";

/** "pessoal--game--claude-2" → "Claude 2" */
export function tabLabel(session: SessionInfo): string {
  const n = session.id.match(/-(\d+)$/)?.[1] ?? "";
  return `${agentLabel[session.agent]} ${n}`.trim();
}

/** Rótulo das visões que viram abas, como "Claude 1". */
const viewLabel: Record<ViewKind, string> = {
  board: "Quadro",
  editor: "Editor",
  preview: "Preview",
};

/** Ícone da aba da visão. */
function ViewIcon({ kind }: { kind: ViewKind }) {
  if (kind === "board") return <LayoutGrid className="size-3.5" />;
  if (kind === "preview") return <Eye className="size-3.5" />;
  return <FileCode2 className="size-3.5" />;
}

export function SessionArea() {
  const {
    hubError,
    sessions,
    activeSession,
    actionError,
    profiles,
    activeProfile,
    activeProject,
    closedTabs,
    boardChat,
    execRequests,
    views,
    activeView,
  } = useAppStore();
  const profile = profiles.find((p) => p.id === activeProfile);
  const color = profile?.color ?? "#a78bfa";
  const claudeWithoutLogin = profile?.claude_login === false;

  if (hubError) {
    return (
      <Empty title="Sem conexão com o hubd" detail={hubError}>
        Ligue a VM com <code className="font-mono">bin/assistente vm iniciar</code> ou, para
        desenvolver, rode <code className="font-mono">./dev just hubd</code>.
      </Empty>
    );
  }
  if (!activeProject || !activeProfile) {
    return <Empty title="Escolha um projeto">As sessões de agente abrem aqui, em abas.</Empty>;
  }

  const key = viewKey(activeProfile, activeProject);
  // Visões são por projeto: as de outro projeto ficam guardadas, sem aparecer aqui.
  const projectViews = views.filter((v) => v.key === key).map((v) => v.kind);
  const focusedView =
    activeView?.key === key && projectViews.includes(activeView.kind) ? activeView.kind : null;

  const projectSessions = sessions.filter(
    (s) => s.profile === activeProfile && s.project === activeProject,
  );
  const visible = visibleProjectSessions({
    activeProfile,
    activeProject,
    sessions,
    closedTabs,
    boardChat,
  });
  const hidden = projectSessions.filter((s) => closedTabs.includes(s.id));
  const lastHidden = hidden.at(-1) ?? null;
  const current = visible.find((s) => s.id === activeSession) ?? visible.at(-1) ?? null;

  /** F12.1: abre o preview do projeto (o dev server) numa aba, sem mexer nas sessões. */
  async function abrirPreview() {
    const cliente = useAppStore.getState().hub;
    const perfil = activeProfile;
    const projeto = activeProject;
    if (!cliente || !perfil || !projeto) return;
    try {
      const view = await cliente.projectSettings(perfil, projeto);
      if (!view.settings.preview_url) {
        useAppStore.setState({
          toast:
            "Este projeto não tem URL de preview. Ponha o endereço do dev server na engrenagem do projeto.",
        });
        return;
      }
      openView(key, "preview");
    } catch (e) {
      useAppStore.setState({
        toast: `não consegui ler as configurações do projeto: ${
          e instanceof Error ? e.message : String(e)
        }`,
      });
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-3 border-b border-line px-5 py-3">
        <h2 className="text-lg font-semibold text-white">{activeProject}</h2>
        <div className="ml-auto flex gap-2">
          {agents.map(({ id, label, icon: Icon, title }) => (
            <button
              key={id}
              type="button"
              onClick={() => openSession(id)}
              title={
                id === "claude" && claudeWithoutLogin
                  ? `Sem login neste perfil: rode ${claudeLoginCommand(profile.id)} no PC`
                  : title
              }
              className={`flex items-center gap-1.5 rounded-lg border bg-white/5 px-3 py-1.5 text-sm transition-colors hover:bg-white/10 ${
                id === "shell" ? "border-amber-500/40 text-amber-200" : "border-line text-slate-200"
              }`}
            >
              <Icon className="size-4" /> {label}
            </button>
          ))}
          <button
            type="button"
            onClick={() => openView(key, "board")}
            title="Quadro de tarefas do projeto: manual ou para o agente (F11)"
            className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-200 transition-colors hover:bg-white/10"
          >
            <LayoutGrid className="size-4" /> Quadro
          </button>
          <button
            type="button"
            onClick={() => openView(key, "editor")}
            title="Editar arquivos do projeto e ver diffs"
            className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-200 transition-colors hover:bg-white/10"
          >
            <FileCode2 className="size-4" /> Editor
          </button>
          <button
            type="button"
            onClick={abrirPreview}
            title="Ver o projeto rodando (dev server), sem sair do app"
            className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-200 transition-colors hover:bg-white/10"
          >
            <Eye className="size-4" /> Visualizar
          </button>
        </div>
      </header>
      {claudeWithoutLogin && !actionError && (
        <p className="border-b border-line bg-amber-950/40 px-5 py-2 text-xs text-amber-200">
          O Claude ainda não tem login no perfil {profile.label} (o login é separado por perfil). No
          PC, rode{" "}
          <code className="rounded bg-black/30 px-1 font-mono">
            {claudeLoginCommand(profile.id)}
          </code>{" "}
          e depois clique em Claude.
        </p>
      )}
      {actionError && (
        <p className="border-b border-line bg-rose-950/40 px-5 py-2 font-mono text-xs text-rose-300">
          {actionError}
        </p>
      )}
      {(projectSessions.length > 0 || projectViews.length > 0) && (
        <div className="flex items-center gap-1 border-b border-line px-3 pt-2" role="tablist">
          {visible.map((session) => {
            const active = focusedView === null && session.id === current?.id;
            const tint = session.agent === "shell" ? outsideJail : color;
            return (
              <div
                key={session.id}
                className={`group flex items-center rounded-t-lg border border-b-0 text-sm ${
                  active
                    ? "bg-[#0b0e1a] text-white"
                    : "border-transparent text-slate-400 hover:text-slate-200"
                }`}
                style={active ? { borderColor: `${tint}88` } : undefined}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={active}
                  onClick={() => selectSession(session.id)}
                  className={`flex flex-1 items-center justify-center gap-1.5 px-3 py-1.5 ${
                    session.agent === "shell" ? "text-amber-200" : ""
                  } ${session.dead ? "opacity-70" : ""}`}
                >
                  {tabLabel(session)}
                  {execRequests.some((r) => r.state === "pending" && r.session === session.id) && (
                    <span
                      className="size-2 rounded-full bg-amber-400"
                      title="Esperando você aprovar um comando fora do jail"
                    />
                  )}
                  {session.dead && (
                    <span
                      className="size-2 rounded-full bg-rose-500"
                      title="Sessão morta: o processo já saiu (o painel ficou com a saída)"
                    />
                  )}
                </button>
                <button
                  type="button"
                  title={
                    session.dead
                      ? "Encerrar a sessão (o processo já saiu)"
                      : "Encerrar a sessão (mata o agente)"
                  }
                  onClick={() => {
                    if (
                      window.confirm(`Encerrar ${tabLabel(session)}? O agente será finalizado.`)
                    ) {
                      endSession(session.id);
                    }
                  }}
                  className="mr-1 rounded p-0.5 text-slate-500 opacity-0 hover:text-rose-300 group-hover:opacity-100"
                >
                  <X className="size-3.5" />
                </button>
              </div>
            );
          })}
          {projectViews.map((kind) => {
            const active = focusedView === kind;
            return (
              <div
                key={kind}
                className={`group flex items-center rounded-t-lg border border-b-0 text-sm ${
                  active
                    ? "bg-[#0b0e1a] text-white"
                    : "border-transparent text-slate-400 hover:text-slate-200"
                }`}
                style={active ? { borderColor: `${color}88` } : undefined}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={active}
                  onClick={() => openView(key, kind)}
                  className="flex flex-1 items-center justify-center gap-1.5 px-3 py-1.5"
                >
                  <ViewIcon kind={kind} />
                  {viewLabel[kind]}
                </button>
                <button
                  type="button"
                  title={`Fechar ${viewLabel[kind]}`}
                  onClick={() => closeView(key, kind)}
                  className="mr-1 rounded p-0.5 text-slate-500 hover:text-white"
                >
                  <X className="size-3.5" />
                </button>
              </div>
            );
          })}
          {lastHidden && (
            <button
              type="button"
              title={`Reabrir ${tabLabel(lastHidden)} (a sessão continuou viva)`}
              onClick={() => reopenTab(lastHidden.id)}
              className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-slate-500 hover:text-slate-200"
            >
              <RotateCcw className="size-3.5" /> {hidden.length}
            </button>
          )}
        </div>
      )}
      {focusedView === "board" ? (
        <KanbanBoard />
      ) : focusedView === "editor" ? (
        <EditorPanel />
      ) : focusedView === "preview" ? (
        <PreviewPanel />
      ) : projectSessions.length === 0 ? (
        <Empty title="Nenhuma sessão aberta">
          Clique em Claude ou OpenCode para abrir um agente neste projeto, ou em Terminal para um
          bash fora do jail. A sessão continua viva na VM mesmo se você fechar o app.
        </Empty>
      ) : current ? (
        <TerminalView key={current.id} sessionId={current.id} />
      ) : (
        <Empty title="Todas as abas fechadas">
          As sessões continuam vivas na VM. Reabra pela seta acima ou por Ctrl+Shift+T para uma
          nova.
        </Empty>
      )}
    </div>
  );
}

function Empty({
  title,
  detail,
  children,
}: {
  title: string;
  detail?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-2 p-10 text-center">
      <p className="text-xl font-semibold text-slate-200">{title}</p>
      {detail && <p className="font-mono text-xs text-rose-300/80">{detail}</p>}
      <p className="max-w-md text-sm text-slate-400">{children}</p>
    </div>
  );
}
