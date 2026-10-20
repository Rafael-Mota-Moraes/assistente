import { Moon, Sunrise } from "lucide-react";
import { useEffect, useState } from "react";
import { HubError } from "../../bridge/hub";
import type { SleepStatus } from "../../bridge/types";
import { useAppStore } from "../../store";
import { openMorning } from "./morning";

const EVERY_MS = 15_000;

function toast(message: string) {
  useAppStore.setState({ toast: message });
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Hora de dormir (F6.3): entrega o projeto ativo ao notebook. Enquanto o `dorme` roda,
 * vira "Dormindo…" com a tentativa atual e oferece a parada (F6.4). */
export function SleepButton() {
  const ready = useAppStore((s) => Boolean(s.hub && s.activeProfile && s.activeProject));
  const profile = useAppStore((s) => s.activeProfile);
  const project = useAppStore((s) => s.activeProject);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<SleepStatus | null>(null);

  useEffect(() => {
    if (!ready || !profile || !project) {
      setStatus(null);
      return;
    }
    let stopped = false;
    async function poll() {
      const { hub } = useAppStore.getState();
      if (!hub) return;
      try {
        const next = await hub.sleepStatus(profile as string, project as string);
        if (!stopped) setStatus(next);
      } catch {
        if (!stopped) setStatus(null);
      }
    }
    void poll();
    const timer = setInterval(poll, EVERY_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [ready, profile, project]);

  async function start() {
    const { hub, activeProfile, activeProject } = useAppStore.getState();
    if (!hub || !activeProfile || !activeProject) return;
    const request = { profile: activeProfile, project: activeProject };
    setBusy(true);
    try {
      const started = await hub.startSleep(request);
      toast(`Dormindo… noite de ${started.date} (${started.session}). Reveja amanhã no "Bom dia".`);
    } catch (error) {
      // Árvore suja: o hubd só dorme com commit; oferecer o WIP é o "ou oferece commit WIP".
      if (
        error instanceof HubError &&
        error.status === 409 &&
        error.message.includes("árvore suja")
      ) {
        // Recusou o WIP: desistência de propósito, sem toast redundante.
        if (!window.confirm(`${error.message}\n\nCommitar tudo como WIP e dormir?`)) return;
        try {
          const started = await hub.startSleep({ ...request, wip: true });
          toast(`Dormindo… noite de ${started.date} (com commit WIP).`);
        } catch (retryError) {
          toast(message(retryError));
        }
      } else {
        toast(message(error));
      }
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    const { hub } = useAppStore.getState();
    if (!hub || !window.confirm("Pedir parada? A tarefa em curso ainda vai terminar.")) return;
    try {
      await hub.stopSleep();
      toast("Parada pedida: o dorme encerra depois da tarefa em curso.");
      setStatus(null);
    } catch (error) {
      toast(message(error));
    }
  }

  if (status?.sleeping) {
    const attempt = [...(status.log_tail ?? "").matchAll(/tarefa: tentativa (\d+\/\d+)/g)].at(
      -1,
    )?.[1];
    return (
      <button
        type="button"
        onClick={stop}
        title="Pedir parada ao dorme; a tarefa em curso termina (F6.4)"
        className="flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-indigo-600/70 to-violet-600/70 px-3 py-2.5 text-sm font-semibold text-white shadow-lg shadow-indigo-900/30"
      >
        <Moon className="size-4 animate-pulse" /> Dormindo…
        {attempt ? ` (tentativa ${attempt})` : ""}
      </button>
    );
  }

  return (
    <button
      type="button"
      disabled={!ready || busy}
      onClick={start}
      title="Entregar o projeto ativo ao notebook: o agente trabalha no backlog enquanto você dorme"
      className="flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-violet-600/60 to-sky-600/60 px-3 py-2.5 text-sm font-semibold text-white shadow-lg shadow-violet-900/30 disabled:opacity-60"
    >
      <Moon className="size-4" /> Hora de dormir
    </button>
  );
}

/** Abre a tela "Bom dia" (F6.6) para o projeto ativo: recolher, revisar e mesclar. */
export function MorningButton() {
  const ready = useAppStore((s) => Boolean(s.hub && s.activeProfile && s.activeProject));
  return (
    <button
      type="button"
      disabled={!ready}
      onClick={() => openMorning()}
      title="Recolher o que o agente fez no notebook e mesclar (F6.5/F6.6)"
      className="flex w-full items-center justify-center gap-2 rounded-xl border border-line bg-white/5 px-3 py-2 text-sm font-medium text-slate-300 transition-colors hover:bg-white/10 disabled:opacity-50"
    >
      <Sunrise className="size-4" /> Bom dia
    </button>
  );
}
