import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";
import { connectTerminal } from "../../bridge/terminal";
import { useAppStore } from "../../store";

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

  useEffect(() => {
    if (!host.current || !hub) return;
    setEnded(null);
    const term = new Terminal({
      theme,
      fontFamily: '"JetBrains Mono", "Fira Code", ui-monospace, monospace',
      fontSize: 13,
      cursorBlink: true,
      allowProposedApi: false,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current);
    fit.fit();

    let disposed = false;
    let connection: Awaited<ReturnType<typeof connectTerminal>> | null = null;
    connectTerminal(hub.terminalUrl(sessionId), {
      onData: (bytes) => term.write(bytes),
      onClose: (reason, detail) => {
        if (!disposed)
          setEnded(reason === "exit" ? "Sessão encerrada." : `Desconectado: ${detail ?? ""}`);
      },
    })
      .then((c) => {
        if (disposed) return c.close();
        connection = c;
        c.resize(term.cols, term.rows);
        term.focus();
      })
      .catch((e: Error) => setEnded(e.message));

    const input = term.onData((data) => connection?.write(data));
    const resize = term.onResize(({ cols, rows }) => connection?.resize(cols, rows));
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(host.current);

    return () => {
      disposed = true;
      observer.disconnect();
      input.dispose();
      resize.dispose();
      connection?.close();
      term.dispose();
    };
  }, [hub, sessionId]);

  return (
    <div className="relative min-h-0 flex-1 bg-[#0b0e1a] p-2">
      <div ref={host} className="h-full w-full" />
      {ended && (
        <div className="absolute inset-x-0 bottom-4 mx-auto w-fit rounded-lg bg-black/70 px-4 py-2 text-sm text-slate-300">
          {ended}
        </div>
      )}
    </div>
  );
}
