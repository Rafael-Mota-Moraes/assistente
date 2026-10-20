import { Bot, RotateCcw, SquareChevronRight, TerminalSquare, X } from "lucide-react";
import type { ReactNode } from "react";
import type { AgentKind, SessionInfo } from "../../bridge/types";
import { useAppStore, visibleProjectSessions } from "../../store";
import { endSession, openSession } from "./actions";
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
    execRequests,
  } = useAppStore();
  const color = profiles.find((p) => p.id === activeProfile)?.color ?? "#a78bfa";

  if (hubError) {
    return (
      <Empty title="Sem conexão com o hubd" detail={hubError}>
        Ligue a VM com <code className="font-mono">bin/assistente vm iniciar</code> ou, para
        desenvolver, rode <code className="font-mono">./dev just hubd</code>.
      </Empty>
    );
  }
  if (!activeProject) {
    return <Empty title="Escolha um projeto">As sessões de agente abrem aqui, em abas.</Empty>;
  }

  const projectSessions = sessions.filter(
    (s) => s.profile === activeProfile && s.project === activeProject,
  );
  const visible = visibleProjectSessions({
    activeProfile,
    activeProject,
    sessions,
    closedTabs,
  });
  const hidden = projectSessions.filter((s) => closedTabs.includes(s.id));
  const lastHidden = hidden.at(-1) ?? null;
  const current = visible.find((s) => s.id === activeSession) ?? visible.at(-1) ?? null;

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
              title={title}
              className={`flex items-center gap-1.5 rounded-lg border bg-white/5 px-3 py-1.5 text-sm transition-colors hover:bg-white/10 ${
                id === "shell" ? "border-amber-500/40 text-amber-200" : "border-line text-slate-200"
              }`}
            >
              <Icon className="size-4" /> {label}
            </button>
          ))}
        </div>
      </header>
      {actionError && (
        <p className="border-b border-line bg-rose-950/40 px-5 py-2 font-mono text-xs text-rose-300">
          {actionError}
        </p>
      )}
      {projectSessions.length === 0 ? (
        <Empty title="Nenhuma sessão aberta">
          Clique em Claude ou OpenCode para abrir um agente neste projeto, ou em Terminal para um
          bash fora do jail. A sessão continua viva na VM mesmo se você fechar o app.
        </Empty>
      ) : (
        <>
          <div className="flex items-center gap-1 border-b border-line px-3 pt-2" role="tablist">
            {visible.map((session) => {
              const active = session.id === current?.id;
              const tint = session.agent === "shell" ? outsideJail : color;
              return (
                <div
                  key={session.id}
                  className={`group flex items-center gap-1 rounded-t-lg border border-b-0 px-3 py-1.5 text-sm ${
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
                    onClick={() => useAppStore.setState({ activeSession: session.id })}
                    className={session.agent === "shell" ? "text-amber-200" : undefined}
                  >
                    {tabLabel(session)}
                  </button>
                  {execRequests.some((r) => r.state === "pending" && r.session === session.id) && (
                    <span
                      className="size-2 rounded-full bg-amber-400"
                      title="Esperando você aprovar um comando fora do jail"
                    />
                  )}
                  <button
                    type="button"
                    title="Encerrar a sessão (mata o agente)"
                    onClick={() => {
                      if (
                        window.confirm(`Encerrar ${tabLabel(session)}? O agente será finalizado.`)
                      ) {
                        endSession(session.id);
                      }
                    }}
                    className="rounded p-0.5 text-slate-500 opacity-0 hover:text-rose-300 group-hover:opacity-100"
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
          {current ? (
            <TerminalView key={current.id} sessionId={current.id} />
          ) : (
            <Empty title="Todas as abas fechadas">
              As sessões continuam vivas na VM. Reabra pela seta acima ou por Ctrl+Shift+T para uma
              nova.
            </Empty>
          )}
        </>
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
