import { useEffect } from "react";
import { useAppStore } from "../../store";
import type { MemoryStatus } from "../../bridge/types";

/** Painel operacional do ai-memory (F5.6): paths, saúde, último commit, link para a web UI. */
export function MemoryStatusPanel({
  memory,
  onClose,
}: {
  memory: MemoryStatus;
  onClose: () => void;
}) {
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg rounded-xl border border-line bg-surface-strong p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold text-slate-100">
            ai-memory — estado operacional
          </h2>
          <button
            type="button"
            className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-white/5"
            onClick={onClose}
          >
            Fechar
          </button>
        </div>
        <dl className="space-y-2 text-sm">
          <Row label="saúde" value={memory.up ? "no ar" : "fora do ar"} />
          {memory.last_error && <Row label="erro" value={memory.last_error} />}
          <Row label="porta" value={String(memory.port)} />
          <Row label="data_dir (SQLite)" value={memory.data_dir} />
          <Row label="mirror_dir (wiki)" value={memory.mirror_dir} />
          <Row label="versão do servidor" value={memory.server_version ?? "—"} />
          <Row label="último commit do wiki" value={memory.last_commit ?? "—"} />
          <Row label="verificado às" value={new Date(memory.checked_at * 1000).toLocaleTimeString("pt-BR")} />
        </dl>
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            className="rounded border border-line px-3 py-1.5 text-xs text-slate-200 hover:bg-white/5"
            onClick={() => {
              useAppStore.setState({ memoryPanelOpen: true });
              onClose();
            }}
          >
            Abrir web UI
          </button>
        </div>
      </div>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex gap-2">
      <dt className="w-40 shrink-0 text-slate-400">{label}</dt>
      <dd className="break-all font-mono text-xs text-slate-200">{value}</dd>
    </div>
  );
}