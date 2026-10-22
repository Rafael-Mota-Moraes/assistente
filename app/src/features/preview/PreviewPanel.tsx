import { isTauri } from "@tauri-apps/api/core";
import { Crosshair, Eye, RotateCw, Settings, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  closePreviewPanel,
  openPreviewPanel,
  type PanelRect,
  previewPanelVisible,
  previewPicker,
  updatePreviewPanelBounds,
} from "../../bridge/host";
import { closeView, useAppStore, viewKey } from "../../store";

/** Retângulo do âncora em pixels lógicos (igual ao px do CSS). */
function rectOf(anchor: HTMLElement): PanelRect {
  const rect = anchor.getBoundingClientRect();
  return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
}

function mensagem(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Preview do projeto (F12.1): a página do dev server numa webview filha dentro da janela.
 * O chrome (URL, recarregar, apontar, fechar) é React; a webview só preenche o retângulo
 * do âncora e acompanha o redimensionamento da janela. */
export function PreviewPanel() {
  const hub = useAppStore((s) => s.hub);
  const profile = useAppStore((s) => s.activeProfile);
  const project = useAppStore((s) => s.activeProject);
  /** Modal aberto: a webview é um widget nativo por cima da UI, então precisa sumir. */
  const modalAberto = useAppStore((s) => s.projectSettings !== null);
  const [url, setUrl] = useState<string | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [apontando, setApontando] = useState(false);
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const key = profile && project ? viewKey(profile, project) : "";

  /** Lê a URL do preview das configurações do projeto (F9.2 guarda o endereço). */
  const carregar = useCallback(() => {
    if (!hub || !profile || !project) return;
    let cancelado = false;
    hub
      .projectSettings(profile, project)
      .then((view) => {
        if (cancelado) return;
        setUrl(view.settings.preview_url ?? null);
        setErro(null);
      })
      .catch((e: unknown) => {
        if (!cancelado) setErro(mensagem(e));
      });
    return () => {
      cancelado = true;
    };
  }, [hub, profile, project]);

  useEffect(() => carregar(), [carregar]);

  /** Webview filha: abre, segue o retângulo do âncora e fecha ao sair. */
  useEffect(() => {
    if (!url) return;
    const anchor = anchorRef.current;
    if (!anchor || !isTauri()) return;
    let ultimoErro = "";
    const reportar = (error: unknown, acao: string) => {
      const texto = mensagem(error);
      if (texto === ultimoErro) return;
      ultimoErro = texto;
      useAppStore.setState({ toast: `${acao}: ${texto}` });
    };
    openPreviewPanel(url, rectOf(anchor)).catch((error) =>
      reportar(error, "não consegui abrir o preview"),
    );
    const resync = () => {
      updatePreviewPanelBounds(rectOf(anchor)).catch(() => {});
    };
    const observer = new ResizeObserver(resync);
    observer.observe(anchor);
    window.addEventListener("resize", resync);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", resync);
      closePreviewPanel().catch(() => {});
    };
  }, [url]);

  /** Modal aberto: esconde a webview; ao fechar, mostra de novo e relê a URL. */
  useEffect(() => {
    if (!url || !modalAberto) return;
    previewPanelVisible(false).catch(() => {});
    return () => {
      previewPanelVisible(true).catch(() => {});
      carregar();
    };
  }, [url, modalAberto, carregar]);

  if (!profile || !project) return null;

  function recarregar() {
    const anchor = anchorRef.current;
    if (!url || !anchor) return;
    openPreviewPanel(url, rectOf(anchor)).catch((error) =>
      useAppStore.setState({ toast: `não consegui recarregar: ${mensagem(error)}` }),
    );
  }

  function apontar() {
    const novo = !apontando;
    setApontando(novo);
    previewPicker(novo).catch((error) =>
      useAppStore.setState({ toast: `não consegui apontar: ${mensagem(error)}` }),
    );
    if (novo) {
      useAppStore.setState({
        toast: "Modo apontar ligado na página; a caixa de pedido chega na F12.2.",
      });
    }
  }

  function fechar() {
    closeView(key, "preview");
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <header className="flex items-center gap-3 border-b border-line bg-surface px-4 py-2">
        <span className="flex shrink-0 items-center gap-2 text-sm font-semibold text-white">
          <Eye className="size-4 text-sky-300" /> Preview
        </span>
        <span
          className="min-w-0 flex-1 truncate font-mono text-xs text-slate-400"
          title={url ?? ""}
        >
          {url ?? "sem URL de preview"}
        </span>
        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={recarregar}
            disabled={!url}
            title="Recarregar a página"
            className="rounded-lg p-1.5 text-slate-400 transition-colors hover:bg-white/10 hover:text-white disabled:opacity-40"
          >
            <RotateCw className="size-4" />
          </button>
          <button
            type="button"
            onClick={apontar}
            disabled={!url}
            title="Apontar um elemento da página"
            className={`rounded-lg p-1.5 transition-colors hover:bg-white/10 disabled:opacity-40 ${
              apontando ? "text-amber-300" : "text-slate-400 hover:text-white"
            }`}
          >
            <Crosshair className="size-4" />
          </button>
          <button
            type="button"
            onClick={fechar}
            title="Fechar (Esc)"
            className="rounded-lg p-1.5 text-slate-400 transition-colors hover:bg-white/10 hover:text-white"
          >
            <X className="size-4" />
          </button>
        </div>
      </header>
      {erro && (
        <p className="border-b border-line bg-rose-950/40 px-4 py-2 font-mono text-xs text-rose-300">
          {erro}
        </p>
      )}
      <div ref={anchorRef} className="min-h-0 flex-1 bg-white">
        {url ? (
          isTauri() ? (
            // Enquanto a webview filha termina de carregar, este cartão fica por baixo dela.
            <div className="flex h-full items-center justify-center text-sm text-slate-400">
              Carregando o preview…
            </div>
          ) : (
            <iframe
              src={url}
              title="preview do projeto"
              className="h-full w-full border-0"
              sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-modals"
            />
          )
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-3 p-10 text-center">
            <p className="text-sm text-slate-400">
              Este projeto não tem URL de preview. Ponha o endereço do dev server na engrenagem do
              projeto — lembrando que o endereço é visto do PC (projeto rodando na VM usa o IP
              dela).
            </p>
            <button
              type="button"
              onClick={() => useAppStore.setState({ projectSettings: { profile, project } })}
              className="flex items-center gap-2 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-200 transition-colors hover:bg-white/10"
            >
              <Settings className="size-4" /> Abrir as configurações do projeto
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
