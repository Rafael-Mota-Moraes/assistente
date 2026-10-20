import { FolderGit2, Moon, Settings, SquareArrowOutUpRight } from "lucide-react";
import { useEffect } from "react";
import { openInCursor } from "../../bridge/host";
import { useAppStore } from "../../store";

export function ProjectList() {
  const { hub, activeProfile, projects, activeProject, health, sessions, execRequests } =
    useAppStore();

  useEffect(() => {
    if (!hub || !activeProfile || !health) return;
    let cancelled = false;
    hub
      .projects(activeProfile)
      .then((projects) => {
        if (!cancelled) useAppStore.setState({ projects });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [hub, activeProfile, health]);

  if (projects.length === 0) {
    return (
      <p className="px-3 text-sm text-slate-500">Nenhum repositório git na raiz deste perfil.</p>
    );
  }

  return (
    <ul className="flex flex-col gap-0.5">
      {projects.map((project) => {
        const active = project.name === activeProject;
        return (
          <li key={project.path} className="group flex items-center">
            <button
              type="button"
              onClick={() => useAppStore.setState({ activeProject: project.name })}
              className={`flex flex-1 items-center gap-2 rounded-lg px-3 py-2 text-left text-sm transition-colors ${
                active ? "bg-white/10 text-white" : "text-slate-300 hover:bg-white/5"
              }`}
            >
              <FolderGit2 className="size-4 shrink-0 text-slate-500" />
              <span className="truncate">{project.name}</span>
              <SessionCount
                count={
                  sessions.filter((s) => s.profile === activeProfile && s.project === project.name)
                    .length
                }
              />
              <PendingCount
                count={
                  execRequests.filter(
                    (r) =>
                      r.state === "pending" &&
                      r.profile === activeProfile &&
                      r.project === project.name,
                  ).length
                }
              />
              {project.has_project_config && (
                <Moon
                  className="ml-auto size-3.5 text-violet-400"
                  aria-label="pronto p/ modo dormir"
                />
              )}
            </button>
            <button
              type="button"
              title="Configurações do projeto (jail)"
              onClick={() =>
                activeProfile &&
                useAppStore.setState({
                  projectSettings: { profile: activeProfile, project: project.name },
                })
              }
              className="invisible rounded-md p-1.5 text-slate-500 hover:text-white group-hover:visible"
            >
              <Settings className="size-3.5" />
            </button>
            <button
              type="button"
              title="Abrir no Cursor"
              onClick={() =>
                openInCursor(project.path).catch((error: unknown) =>
                  useAppStore.setState({
                    toast: error instanceof Error ? error.message : String(error),
                  }),
                )
              }
              className="invisible rounded-md p-1.5 text-slate-500 hover:text-white group-hover:visible"
            >
              <SquareArrowOutUpRight className="size-3.5" />
            </button>
          </li>
        );
      })}
    </ul>
  );
}

/** Pedidos fora do jail esperando você (F9.7). */
function PendingCount({ count }: { count: number }) {
  if (count === 0) return null;
  return (
    <span
      className="rounded-full bg-amber-500/25 px-1.5 text-[10px] font-semibold text-amber-200"
      title={`${count} pedido(s) para rodar fora do jail`}
    >
      {count}
    </span>
  );
}

function SessionCount({ count }: { count: number }) {
  if (count === 0) return null;
  return (
    <span
      className="rounded-full bg-emerald-500/20 px-1.5 text-[10px] font-semibold text-emerald-300"
      title={`${count} sessão(ões) aberta(s)`}
    >
      {count}
    </span>
  );
}
