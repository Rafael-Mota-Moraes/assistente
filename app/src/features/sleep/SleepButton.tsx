import { Moon } from "lucide-react";

/** Modo "hora de dormir" (F6): entrega o projeto ao notebook e acompanha de manhã. */
export function SleepButton() {
  return (
    <button
      type="button"
      disabled
      title="Hora de dormir — em breve (F6)"
      className="flex w-full items-center justify-center gap-2 rounded-xl bg-gradient-to-r from-violet-600/60 to-sky-600/60 px-3 py-2.5 text-sm font-semibold text-white shadow-lg shadow-violet-900/30 disabled:opacity-60"
    >
      <Moon className="size-4" /> Hora de dormir
    </button>
  );
}
