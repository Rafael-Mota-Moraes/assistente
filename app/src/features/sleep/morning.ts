// Tela "Bom dia" (F6.6): recolher o que o agente fez no notebook e mesclar sem abrir o
// terminal. O estado vive em `store.morning`; o log ao vivo entra aqui quando a F6.3
// (GET /api/sleep) existir.
import { useAppStore } from "../../store";

/** Data local de ontem (`YYYY-MM-DD`): a noite típica do modo dormir. */
export function yesterday(): string {
  const d = new Date();
  d.setDate(d.getDate() - 1);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function fail(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function patch(values: Partial<NonNullable<ReturnType<typeof useAppStore.getState>["morning"]>>) {
  const morning = useAppStore.getState().morning;
  if (!morning) return;
  useAppStore.setState({ morning: { ...morning, ...values } });
}

async function collect(profile: string, project: string, date: string) {
  const { hub } = useAppStore.getState();
  if (!hub) return;
  patch({ busy: true });
  try {
    const report = await hub.collectSleep({ profile, project, date });
    patch({ report, error: null, busy: false });
  } catch (error) {
    patch({ error: fail(error), busy: false });
  }
}

/** Abre o modal para o projeto ativo e recolhe a noite `date` (padrão: ontem). */
export async function openMorning(date: string = yesterday()) {
  const { hub, activeProfile, activeProject } = useAppStore.getState();
  if (!hub || !activeProfile || !activeProject) return;
  useAppStore.setState({
    morning: {
      profile: activeProfile,
      project: activeProject,
      date,
      report: null,
      error: null,
      merged: false,
      busy: true,
    },
  });
  await collect(activeProfile, activeProject, date);
}

/** Troca a noite e recolhe de novo (o merge anterior deixa de valer). */
export async function setMorningDate(date: string) {
  const { morning } = useAppStore.getState();
  if (!morning || date === morning.date) return;
  patch({ date, report: null, error: null, merged: false });
  await collect(morning.profile, morning.project, date);
}

/** Re-colhe a mesma noite (depois de erro, ou para atualizar enquanto o agente trabalha). */
export async function refreshMorning() {
  const { morning } = useAppStore.getState();
  if (!morning) return;
  await collect(morning.profile, morning.project, morning.date);
}

/** Merge fast-forward com um clique; 409 (a base andou) vira aviso, não exceção. */
export async function mergeMorning() {
  const { hub, morning } = useAppStore.getState();
  if (!hub || !morning?.report?.fast_forward || morning.merged) return;
  patch({ busy: true });
  try {
    await hub.mergeSleep({
      profile: morning.profile,
      project: morning.project,
      date: morning.date,
    });
    patch({ merged: true, error: null, busy: false });
  } catch (error) {
    patch({ error: fail(error), busy: false });
  }
}

export function closeMorning() {
  useAppStore.setState({ morning: null });
}
