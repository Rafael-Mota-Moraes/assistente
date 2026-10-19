import { FolderGit2, Moon, SquareArrowOutUpRight } from "lucide-react";
import { useEffect } from "react";
import { openInCursor } from "../../bridge/host";
import { useAppStore } from "../../store";

export function ProjectList() {
  const { hub, activeProfile, projects, activeProject, health } = useAppStore();

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
              {project.has_project_config && (
                <Moon
                  className="ml-auto size-3.5 text-violet-400"
                  aria-label="pronto p/ modo dormir"
                />
              )}
            </button>
            <button
              type="button"
              title="Abrir no Cursor"
              onClick={() => openInCursor(project.path).catch(() => {})}
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
