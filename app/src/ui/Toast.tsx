import { useEffect } from "react";
import { useAppStore } from "../store";

const DURATION_MS = 6000;

/** Aviso temporário no rodapé (erros de ações do host, como abrir o Cursor). Some sozinho. */
export function Toast() {
  const toast = useAppStore((s) => s.toast);

  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => useAppStore.setState({ toast: null }), DURATION_MS);
    return () => clearTimeout(timer);
  }, [toast]);

  if (!toast) return null;
  return (
    <div
      role="status"
      className="pointer-events-none fixed bottom-14 left-1/2 z-50 w-fit max-w-[90vw] -translate-x-1/2"
    >
      <p className="rounded-lg border border-rose-900/60 bg-rose-950/90 px-4 py-2 font-mono text-xs text-rose-200 shadow-lg">
        {toast}
      </p>
    </div>
  );
}
