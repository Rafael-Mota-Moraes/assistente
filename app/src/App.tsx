import { ProfileSwitcher } from "./features/profiles/ProfileSwitcher";
import { ProjectList } from "./features/projects/ProjectList";
import { SessionArea } from "./features/sessions/SessionArea";
import { SleepButton } from "./features/sleep/SleepButton";
import { StatusBar } from "./features/status/StatusBar";
import { usePolling } from "./features/status/usePolling";

export function App() {
  usePolling();
  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1">
        <aside className="flex w-72 shrink-0 flex-col gap-4 border-r border-line bg-surface p-4 backdrop-blur">
          <h1 className="flex items-center gap-2 px-1 text-lg font-bold tracking-tight text-white">
            <span className="size-2.5 rounded-full bg-gradient-to-br from-violet-400 to-sky-400" />
            Assistente
          </h1>
          <ProfileSwitcher />
          <nav className="min-h-0 flex-1 overflow-y-auto">
            <ProjectList />
          </nav>
          <SleepButton />
        </aside>
        <main className="flex min-w-0 flex-1 flex-col">
          <SessionArea />
        </main>
      </div>
      <StatusBar />
    </div>
  );
}
