import { Bot, TerminalSquare } from "lucide-react";
import type { ReactNode } from "react";
import { useAppStore } from "../../store";

const agents = [
  { id: "claude", label: "Claude", icon: Bot },
  { id: "opencode", label: "OpenCode", icon: TerminalSquare },
] as const;

/** Placeholder até a F2: terminais tmux via WebSocket com xterm.js. */
export function SessionArea() {
  const { activeProject, hubError } = useAppStore();

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
  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line px-5 py-3">
        <h2 className="text-lg font-semibold text-white">{activeProject}</h2>
        <div className="ml-auto flex gap-2">
          {agents.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              disabled
              title="Em breve (F2.2)"
              className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-300 disabled:opacity-50"
            >
              <Icon className="size-4" /> {label}
            </button>
          ))}
        </div>
      </header>
      <Empty title="Nenhuma sessão aberta">
        Os terminais do Claude e do OpenCode (rodando no ai-jail dentro da VM) chegam na F2.
      </Empty>
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
