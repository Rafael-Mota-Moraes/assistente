import { Bot, TerminalSquare, X } from "lucide-react";
import type { ReactNode } from "react";
import type { AgentKind, SessionInfo } from "../../bridge/types";
import { useAppStore } from "../../store";
import { endSession, openSession } from "./actions";
import { TerminalView } from "./TerminalView";

const agents: { id: AgentKind; label: string; icon: typeof Bot }[] = [
  { id: "claude", label: "Claude", icon: Bot },
  { id: "opencode", label: "OpenCode", icon: TerminalSquare },
];

const agentLabel: Record<AgentKind, string> = { claude: "Claude", opencode: "OpenCode" };

/** "pessoal--game--claude-2" → "Claude 2" */
export function tabLabel(session: SessionInfo): string {
  const n = session.id.match(/-(\d+)$/)?.[1] ?? "";
  return `${agentLabel[session.agent]} ${n}`.trim();
}

export function SessionArea() {
  const { activeProject, activeProfile, hubError, sessions, activeSession, actionError, profiles } =
    useAppStore();
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
  const current =
    projectSessions.find((s) => s.id === activeSession) ?? projectSessions.at(-1) ?? null;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-3 border-b border-line px-5 py-3">
        <h2 className="text-lg font-semibold text-white">{activeProject}</h2>
        <div className="ml-auto flex gap-2">
          {agents.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              onClick={() => openSession(id)}
              title={`Nova sessão do ${label} (no ai-jail da VM)`}
              className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-200 transition-colors hover:bg-white/10"
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
          Clique em Claude ou OpenCode para abrir um agente neste projeto. A sessão continua viva na
          VM mesmo se você fechar o app.
        </Empty>
      ) : (
        <>
          <div className="flex gap-1 border-b border-line px-3 pt-2" role="tablist">
            {projectSessions.map((session) => {
              const active = session.id === current?.id;
              return (
                <div
                  key={session.id}
                  className={`group flex items-center gap-1 rounded-t-lg border border-b-0 px-3 py-1.5 text-sm ${
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
                    onClick={() => useAppStore.setState({ activeSession: session.id })}
                  >
                    {tabLabel(session)}
                  </button>
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
          </div>
          {current && <TerminalView key={current.id} sessionId={current.id} />}
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
