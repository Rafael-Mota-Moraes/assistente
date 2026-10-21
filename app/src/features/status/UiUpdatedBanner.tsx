import { X } from "lucide-react";
import { useEffect, useState } from "react";
import {
  clearUiUpdateNotice,
  readUiUpdateNotice,
  shortUiHash,
  type UiUpdateNotice,
} from "./uiUpdate";

const DURATION_MS = 8_000;

/** F8.7: depois do reload, avisa que a UI em uso mudou. Se a casca mudou, pede reinstalar. */
export function UiUpdatedBanner() {
  const [notice, setNotice] = useState<UiUpdateNotice | null>(() => readUiUpdateNotice());

  function esconder() {
    clearUiUpdateNotice();
    setNotice(null);
  }

  // Sem mudança na casca o aviso é só informativo; com a casca, fica até fechar.
  useEffect(() => {
    if (!notice || notice.shell) return;
    const timer = setTimeout(() => {
      clearUiUpdateNotice();
      setNotice(null);
    }, DURATION_MS);
    return () => clearTimeout(timer);
  }, [notice]);

  if (!notice) return null;
  return (
    <div
      role="status"
      className={`flex shrink-0 items-center gap-3 border-b px-4 py-2 text-sm ${
        notice.shell
          ? "border-amber-500/40 bg-amber-500/10 text-amber-100"
          : "border-emerald-500/30 bg-emerald-500/10 text-emerald-100"
      }`}
    >
      <p className="min-w-0 flex-1">
        Versão nova aplicada (<span className="font-mono">{shortUiHash(notice.commit)}</span>).
        {notice.shell ? (
          <>
            {" "}
            A casca do app mudou: reinstale com{" "}
            <code className="rounded bg-black/30 px-1 font-mono text-xs">assistente atualizar</code>
            .
          </>
        ) : null}
      </p>
      <button
        type="button"
        aria-label="Fechar aviso"
        onClick={esconder}
        className="rounded p-1 hover:bg-white/10"
      >
        <X className="size-4" />
      </button>
    </div>
  );
}
