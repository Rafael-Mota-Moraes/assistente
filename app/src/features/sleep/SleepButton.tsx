import { Moon, Sunrise } from "lucide-react";
import { useAppStore } from "../../store";
import { openMorning } from "./morning";

/** Modo "hora de dormir" (F6): entrega o projeto ao notebook e acompanha de manhã. */
export function SleepButton() {
  return (
    <button
      type="button"
      disabled
      title="Hora de dormir — em breve (F6.3)"
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
