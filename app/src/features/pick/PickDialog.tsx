import { Crosshair, X } from "lucide-react";
import { useState } from "react";
import { PickCodeContext } from "./PickCodeContext";
import { buildPickPrompt } from "./prompt";
import type { PickElement } from "./types";

/** Caixa de pedido do modo apontar (F12.2).
 *
 * Mostra o elemento apontado — com o arquivo:linha quando a página expõe a fonte (F12.3) — e o
 * texto que vai para o agente (F12.4 manda). */
export function PickDialog({
  pick,
  projectRoot,
  onClose,
  onSend,
}: {
  pick: PickElement | null;
  projectRoot?: string | null;
  onClose: () => void;
  onSend?: (request: string, prompt: string) => void;
}) {
  const [request, setRequest] = useState("");
  if (!pick) return null;
  const pedido = request.trim();
  const prompt = buildPickPrompt(pick, pedido, projectRoot);

  return (
    <div
      role="dialog"
      aria-label="Apontar na página"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") onClose();
      }}
    >
      <div className="flex max-h-full w-full max-w-xl flex-col gap-3 overflow-y-auto rounded-2xl border border-line bg-surface-strong p-6 shadow-2xl">
        <header className="flex items-center gap-3">
          <Crosshair className="size-5 shrink-0 text-violet-300" />
          <h2 className="text-lg font-semibold text-white">Apontar</h2>
          <span className="truncate text-xs text-slate-400">{pick.url}</span>
          <button
            type="button"
            title="Fechar"
            onClick={onClose}
            className="ml-auto rounded-md p-1.5 text-slate-400 hover:bg-white/10 hover:text-white"
          >
            <X className="size-4" />
          </button>
        </header>

        <p className="text-sm text-slate-300">
          <span className="font-mono text-violet-200">{pick.tag}</span>
          {pick.selector ? (
            <span className="font-mono text-slate-400"> · {pick.selector}</span>
          ) : null}
          {pick.text ? <span className="text-slate-400"> · "{pick.text}"</span> : null}
        </p>

        <PickCodeContext pick={pick} projectRoot={projectRoot} />

        <label className="flex flex-col gap-1 text-sm">
          <span className="font-medium text-slate-200">O que você quer mudar?</span>
          <textarea
            aria-label="O que você quer mudar?"
            value={request}
            rows={3}
            placeholder="ex.: deixa esse botão verde"
            onChange={(event) => setRequest(event.target.value)}
            className="w-full resize-y rounded-lg border border-line bg-black/30 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
          />
        </label>

        <details className="text-xs text-slate-400">
          <summary className="cursor-pointer select-none">Texto que vai para o agente</summary>
          <pre className="mt-2 overflow-x-auto rounded-lg bg-black/40 p-3 font-mono whitespace-pre-wrap text-slate-300">
            {prompt}
          </pre>
        </details>

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg px-3 py-1.5 text-sm text-slate-300 hover:bg-white/10"
          >
            Cancelar
          </button>
          <button
            type="button"
            disabled={!pedido || !onSend}
            onClick={() => onSend?.(pedido, prompt)}
            className="rounded-lg bg-violet-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-violet-500 disabled:opacity-50"
          >
            Enviar
          </button>
        </div>
      </div>
    </div>
  );
}
