import { openUrl } from "../../bridge/host";
import { useAppStore } from "../../store";

/** Painel embutido da web UI do ai-memory (F5.5). Abre/fecha via `memoryPanelOpen` no store. */
export function MemoryPanel() {
  const { hub, memory, memoryPanelOpen } = useAppStore();
  if (!memoryPanelOpen) return null;

  const url = hub ? `${hub.baseUrl()}/web` : "";

  function close() {
    useAppStore.setState({ memoryPanelOpen: false });
  }

  async function openInBrowser() {
    if (!url) return;
    try {
      await openUrl(url);
    } catch {
      // ignora: o iframe abaixo já mostra a UI
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      role="dialog"
      aria-modal="true"
      aria-label="Memória"
    >
      <div className="flex h-[85vh] w-[90vw] max-w-5xl flex-col overflow-hidden rounded-lg border border-line bg-surface shadow-xl">
        <header className="flex items-center justify-between border-b border-line px-4 py-2">
          <span className="text-sm font-semibold text-white">
            ai-memory{memory ? ` — 127.0.0.1:${memory.port}` : ""}
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="rounded border border-line px-2 py-1 text-xs text-slate-300 hover:bg-white/5"
              onClick={openInBrowser}
              disabled={!url}
            >
              Abrir no navegador
            </button>
            <button
              type="button"
              className="rounded border border-line px-2 py-1 text-xs text-slate-300 hover:bg-white/5"
              onClick={close}
            >
              Fechar
            </button>
          </div>
        </header>
        <div className="min-h-0 flex-1 bg-white">
          {url ? (
            <iframe
              src={url}
              title="ai-memory"
              className="h-full w-full border-0"
              // Permite a web UI do ai-memory funcionar dentro do app.
              sandbox="allow-same-origin allow-scripts allow-forms allow-popups"
            />
          ) : (
            <div className="flex h-full items-center justify-center text-sm text-slate-500">
              hubd desconectado
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
