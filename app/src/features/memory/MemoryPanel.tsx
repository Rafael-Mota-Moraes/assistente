import { isTauri } from "@tauri-apps/api/core";
import { Brain, ExternalLink, X } from "lucide-react";
import { useEffect, useRef } from "react";
import {
  closeMemoryPanel,
  openMemoryPanel,
  openUrl,
  type PanelRect,
  updateMemoryPanelBounds,
} from "../../bridge/host";
import { anyModalOpen, useAppStore } from "../../store";

/** Retângulo do âncora em pixels lógicos (igual ao px do CSS). */
function rectOf(anchor: HTMLElement): PanelRect {
  const rect = anchor.getBoundingClientRect();
  return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
}

/** Painel da web UI do ai-memory (F5.5) DENTRO da janela principal: a barra de título é
 * React e o conteúdo é uma webview filha apoiada sobre o âncora (coordenadas via Rust). No
 * navegador (pnpm dev) cai num iframe comum — lá cookie de terceiros até passa. */
export function MemoryPanel() {
  const hub = useAppStore((s) => s.hub);
  const memory = useAppStore((s) => s.memory);
  const open = useAppStore((s) => s.memoryPanelOpen);
  const modalOpen = useAppStore(anyModalOpen);
  const pending = useAppStore((s) =>
    s.execRequests
      .filter((r) => r.state === "pending")
      .map((r) => r.id)
      .join(","),
  );
  const anchorRef = useRef<HTMLDivElement | null>(null);
  /** Modais React (Bom dia, configurações, Assistente, estado do ai-memory, caixa do apontar)
   * e cartões de aprovação ficariam ATRÁS da webview filha; então sobrepõem → fecha o painel
   * (F12.6). Aqui o painel fecha (a webview é o painel inteiro); no preview (F12.1), que tem
   * chrome em volta, a webview é que some e volta. `seen` guarda o que já estava pendente no
   * instante da abertura: pendência nova sim fecha o painel. O aviso (toast) não fecha: some
   * sozinho, e os erros do próprio painel viram aviso. */
  const seen = useRef<string[] | null>(null);

  useEffect(() => {
    if (!open) {
      seen.current = null;
      return;
    }
    if (seen.current === null) seen.current = pending.split(",").filter(Boolean);
    if (modalOpen) {
      useAppStore.setState({ memoryPanelOpen: false });
      return;
    }
    const known = seen.current;
    if (pending.split(",").some((id) => id && !known.includes(id))) {
      useAppStore.setState({ memoryPanelOpen: false });
    }
  }, [open, modalOpen, pending]);

  const url = hub ? hub.memoryLoginUrl() : "";

  function close() {
    useAppStore.setState({ memoryPanelOpen: false });
  }

  /** Webview filha: posiciona, escala com o âncora (janela/redimensionamento) e fecha. */
  useEffect(() => {
    if (!open) return;
    const anchor = anchorRef.current;
    if (!anchor || !isTauri()) return;
    let lastError = "";
    const report = (error: unknown, action: string) => {
      const message = error instanceof Error ? error.message : String(error);
      if (message === lastError) return;
      lastError = message;
      useAppStore.setState({ toast: `${action}: ${message}` });
    };
    openMemoryPanel(url, rectOf(anchor)).catch((error) =>
      report(error, "não consegui abrir a memória"),
    );
    const resync = () => {
      updateMemoryPanelBounds(rectOf(anchor)).catch(() => {});
    };
    const observer = new ResizeObserver(resync);
    observer.observe(anchor);
    window.addEventListener("resize", resync);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", resync);
      closeMemoryPanel().catch(() => {});
    };
  }, [open, url]);

  /** Esc fecha o painel. Dentro da webview da memória, o foco é dela; o X da barra fecha sempre. */
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") useAppStore.setState({ memoryPanelOpen: false });
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open) return null;

  function openInBrowser() {
    if (url) void openUrl(url).catch(() => {});
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <header className="flex items-center justify-between border-b border-line bg-surface px-4 py-2">
        <span className="flex items-center gap-2 text-sm font-semibold text-white">
          <Brain className="size-4 text-violet-300" /> Memória
          {memory ? (
            <span className="font-normal text-slate-400">— wiki do ai-memory na VM</span>
          ) : null}
        </span>
        <div className="flex items-center gap-1">
          {url ? (
            <button
              type="button"
              onClick={openInBrowser}
              title="Abrir a web UI numa aba do navegador"
              className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-2.5 py-1 text-xs font-medium text-slate-300 transition-colors hover:bg-white/10"
            >
              <ExternalLink className="size-3.5" /> Navegador
            </button>
          ) : null}
          <button
            type="button"
            onClick={close}
            title="Fechar (Esc)"
            className="rounded-lg p-1.5 text-slate-400 transition-colors hover:bg-white/10 hover:text-white"
          >
            <X className="size-4" />
          </button>
        </div>
      </header>
      <div ref={anchorRef} className="min-h-0 flex-1 bg-white">
        {url ? (
          isTauri() ? (
            // Enquanto a webview filha termina de carregar, este cartão fica por baixo dela.
            <div className="flex h-full items-center justify-center text-sm text-slate-400">
              Carregando a web UI do ai-memory…
            </div>
          ) : (
            <iframe
              src={url}
              title="ai-memory"
              className="h-full w-full border-0"
              sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-modals"
            />
          )
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-slate-500">
            hubd desconectado
          </div>
        )}
      </div>
    </div>
  );
}
