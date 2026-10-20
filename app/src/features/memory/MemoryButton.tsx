import { Brain } from "lucide-react";
import { openMemoryWindow } from "../../bridge/host";
import { useAppStore } from "../../store";

/** Abre a web UI do ai-memory numa janela do app (F5.5), já autenticada pelo hubd. */
export function MemoryButton() {
  const hub = useAppStore((s) => s.hub);
  const up = useAppStore((s) => Boolean(s.memory?.up));

  async function open() {
    if (!hub) return;
    try {
      await openMemoryWindow(hub.memoryLoginUrl());
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      useAppStore.setState({ toast: `não consegui abrir a memória: ${message}` });
    }
  }

  return (
    <button
      type="button"
      disabled={!hub || !up}
      onClick={open}
      title={
        up
          ? "Ver e editar o que os agentes lembram (wiki do ai-memory)"
          : "ai-memory fora do ar na VM"
      }
      className="flex w-full items-center justify-center gap-2 rounded-xl border border-line bg-white/5 px-3 py-2 text-sm font-medium text-slate-300 transition-colors hover:bg-white/10 disabled:opacity-50"
    >
      <Brain className="size-4" /> Memória
    </button>
  );
}
