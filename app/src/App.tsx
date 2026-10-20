import { ExecApprovals } from "./features/exec/ExecApprovals";
import { ProfileSwitcher } from "./features/profiles/ProfileSwitcher";
import { ProjectList } from "./features/projects/ProjectList";
import { ProjectSettingsDialog } from "./features/projects/ProjectSettingsDialog";
import { SessionArea } from "./features/sessions/SessionArea";
import { useShortcuts } from "./features/sessions/shortcuts";
import { GoodMorning } from "./features/sleep/GoodMorning";
import { MorningButton, SleepButton } from "./features/sleep/SleepButton";
import { StatusBar } from "./features/status/StatusBar";
import { usePolling } from "./features/status/usePolling";
import { Toast } from "./ui/Toast";

export function App() {
  usePolling();
  useShortcuts();
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
          <div className="flex flex-col gap-2">
            <SleepButton />
            <MorningButton />
          </div>
        </aside>
        <main className="flex min-w-0 flex-1 flex-col">
          <SessionArea />
        </main>
      </div>
      <StatusBar />
      <Toast />
      <GoodMorning />
      <ProjectSettingsDialog />
      <ExecApprovals />
    </div>
  );
}
