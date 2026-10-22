import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";
import { openUrl } from "../../bridge/host";
import { connectPersistent } from "../../bridge/terminal";
import { useAppStore } from "../../store";
import {
  clearStaleTextarea,
  clipboardShortcut,
  copyText,
  dedupeComposedInput,
  osc52Text,
  pasteText,
} from "./clipboard";
import { shortcutAction } from "./shortcuts";

const theme = {
  background: "#0b0e1a",
  foreground: "#e2e8f0",
  cursor: "#a78bfa",
  selectionBackground: "#a78bfa55",
  black: "#1e293b",
  brightBlack: "#475569",
  blue: "#7dd3fc",
  brightBlue: "#bae6fd",
  magenta: "#c4b5fd",
  brightMagenta: "#ddd6fe",
};

/** Um terminal xterm.js ligado a uma sessão tmux. Desmontar só desanexa a sessão. */
export function TerminalView({ sessionId }: { sessionId: string }) {
  const host = useRef<HTMLDivElement>(null);
  const hub = useAppStore((s) => s.hub);
  const [ended, setEnded] = useState<string | null>(null);
  const [reconnecting, setReconnecting] = useState<string | null>(null);

  useEffect(() => {
    if (!host.current || !hub) return;
    setEnded(null);
    setReconnecting(null);
    const term = new Terminal({
      theme,
      fontFamily: '"JetBrains Mono", "Fira Code", ui-monospace, monospace',
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: false,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    // Ctrl+clique abre o link no navegador padrão do host (F2.4).
    term.loadAddon(
      new WebLinksAddon((_event, uri) => {
        openUrl(uri).catch((e: Error) => console.error("abrindo o link:", e.message));
      }),
    );
    term.open(host.current);
    fit.fit();
    const releaseTextarea = term.textarea ? clearStaleTextarea(host.current, term.textarea) : null;

    let disposed = false;
    // F8.1: caiu sem `exit` → reconecta sozinho com backoff; a faixa amarela avisa.
    const link = connectPersistent(hub.terminalUrl(sessionId), {
      onData: (bytes) => term.write(bytes),
      onOpen: () => {
        if (disposed) return;
        setReconnecting(null);
        term.focus();
      },
      onReconnecting: (attempt, delayMs) => {
        if (!disposed)
          setReconnecting(`Reconectando… tentativa ${attempt}, em ${Math.round(delayMs / 1000)} s`);
      },
      onExit: (status) => {
        if (disposed) return;
        // F14.3: a sessão morreu e deixou o painel na tela. O aviso traz o status de saída
        // e o que o processo imprimiu fica no terminal. O primeiro aviso fica: encerrar a
        // sessão depois não apaga o status que a morte mostrou.
        setEnded(
          (antes) =>
            antes ??
            (status === undefined ? "Sessão encerrada." : `Sessão encerrada (status ${status}).`),
        );
      },
    });
    link.resize(term.cols, term.rows);

    // Arrastar com o mouse seleciona no tmux (mouse on), que ao soltar manda o texto por OSC 52.
    // Shift+arrastar seleciona no xterm.js, e Ctrl+Shift+C copia essa seleção.
    const clipboardFailed = (e: unknown) => console.error("área de transferência:", e);
    const osc52 = term.parser.registerOscHandler(52, (data) => {
      const text = osc52Text(data);
      if (text) copyText(text).catch(clipboardFailed);
      return true;
    });
    term.attachCustomKeyEventHandler((event) => {
      const action = clipboardShortcut(event);
      if (!action) {
        // Atalhos de abas/perfil: engole aqui (não vão ao agente); quem executa é o
        // listener do window em useShortcuts, quando o evento chega lá.
        if (shortcutAction(event)) {
          event.preventDefault();
          return false;
        }
        return true;
      }
      if (event.type === "keydown") {
        event.preventDefault();
        if (action === "copy" && term.hasSelection())
          copyText(term.getSelection()).catch(clipboardFailed);
        if (action === "paste")
          pasteText()
            .then((text) => term.paste(text))
            .catch(clipboardFailed);
      }
      return false;
    });

    const acceptInput = dedupeComposedInput();
    const input = term.onData((data) => {
      if (acceptInput(data)) link.write(data);
    });
    const resize = term.onResize(({ cols, rows }) => link.resize(cols, rows));
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(host.current);

    return () => {
      disposed = true;
      observer.disconnect();
      releaseTextarea?.();
      osc52.dispose();
      input.dispose();
      resize.dispose();
      link.close();
      term.dispose();
    };
  }, [hub, sessionId]);

  return (
    <div className="relative min-h-0 flex-1 bg-[#0b0e1a] p-2">
      <div ref={host} className="h-full w-full" />
      {reconnecting && !ended && (
        <div className="absolute inset-x-0 top-4 mx-auto w-fit rounded-lg bg-amber-950/80 px-4 py-2 text-sm text-amber-200">
          {reconnecting}
        </div>
      )}
      {ended && (
        <div className="absolute inset-x-0 bottom-4 mx-auto w-fit rounded-lg bg-black/70 px-4 py-2 text-sm text-slate-300">
          {ended}
        </div>
      )}
    </div>
  );
}
