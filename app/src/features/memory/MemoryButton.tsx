import { Brain } from "lucide-react";
import { useAppStore } from "../../store";

/** Abre a web UI do ai-memory num painel embutido na área principal (F5.5). Aberta, o botão
 * vira atalho de fechar; o painel em si tem barra própria com ✕ (Esc também fecha). */
export function MemoryButton() {
  const hub = useAppStore((s) => s.hub);
  const up = useAppStore((s) => Boolean(s.memory?.up));
  const open = useAppStore((s) => s.memoryPanelOpen);

  function toggle() {
    if (!hub || !up) return;
    useAppStore.setState({ memoryPanelOpen: !open });
  }

  return (
    <button
      type="button"
      disabled={!hub || !up}
      onClick={toggle}
      title={
        up
          ? open
            ? "Fechar o painel da memória"
            : "Ver e editar o que os agentes lembram (wiki do ai-memory)"
          : "ai-memory fora do ar na VM"
      }
      className={
        open
          ? "flex w-full items-center justify-center gap-2 rounded-xl border border-violet-400/60 bg-violet-500/15 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-violet-500/25"
          : "flex w-full items-center justify-center gap-2 rounded-xl border border-line bg-white/5 px-3 py-2 text-sm font-medium text-slate-300 transition-colors hover:bg-white/10 disabled:opacity-50"
      }
    >
      <Brain className="size-4" /> Memória
    </button>
  );
}
