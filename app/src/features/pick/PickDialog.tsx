import { Crosshair, X } from "lucide-react";
import { useEffect, useState } from "react";
import { listenPreviewPick } from "../../bridge/host";
import { useAppStore } from "../../store";

/** Caixa de pedido do modo apontar (F12.2): mostra o elemento escolhido na página do
 *  preview e o espaço para escrever o que deve mudar. O clique foi capturado na fase
 *  de captura, então a ação do botão da página não disparou. O envio ao agente é a F12.4. */
export function PickDialog() {
  const pick = useAppStore((s) => s.pick);
  const [request, setRequest] = useState("");

  // O Rust intercepta a navegação `assistente-pick:` da webview do preview e emite o
  // evento; aqui o pick vira a caixa de pedido.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    void listenPreviewPick((picked) => useAppStore.setState({ pick: picked })).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, []);

  // Um apontamento novo começa com o pedido em branco.
  useEffect(() => {
    if (pick) setRequest("");
  }, [pick]);

  if (!pick) return null;

  function close() {
    useAppStore.setState({ pick: null });
  }

  const aria = Object.entries(pick.aria);

  return (
    <div
      role="dialog"
      aria-label="Apontar elemento"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onKeyDown={(event) => {
        if (event.key === "Escape") close();
      }}
    >
      <div className="flex max-h-full w-full max-w-2xl flex-col gap-4 overflow-y-auto rounded-2xl border border-line bg-surface-strong p-6 shadow-2xl">
        <header className="flex items-center gap-3">
          <Crosshair className="size-5 text-violet-300" />
          <h2 className="text-lg font-semibold text-white">Apontar</h2>
          <button
            type="button"
            onClick={close}
            title="Fechar"
            className="ml-auto rounded p-1 text-slate-400 hover:text-white"
          >
            <X className="size-4" />
          </button>
        </header>

        <p className="text-sm text-slate-400">
          O elemento escolhido na página do preview. O clique foi interceptado: a ação do botão da
          página não disparou.
        </p>

        <dl className="grid grid-cols-[6.5rem_1fr] gap-x-4 gap-y-1.5 text-sm">
          <dt className="text-slate-400">Elemento</dt>
          <dd className="font-mono text-white">
            {pick.tag}
            {pick.id ? <span className="text-slate-400">#{pick.id}</span> : null}
            {pick.classes.length > 0 ? (
              <span className="text-slate-400">.{pick.classes.join(".")}</span>
            ) : null}
          </dd>

          <dt className="text-slate-400">Texto</dt>
          <dd className="text-white">{pick.text ?? "—"}</dd>

          <dt className="text-slate-400">Seletor</dt>
          <dd className="font-mono text-violet-200">{pick.selector}</dd>

          <dt className="text-slate-400">Caminho</dt>
          <dd className="font-mono text-slate-300">{pick.dom_path}</dd>

          {pick.role ? (
            <>
              <dt className="text-slate-400">Role</dt>
              <dd className="font-mono text-slate-300">{pick.role}</dd>
            </>
          ) : null}

          {aria.length > 0 ? (
            <>
              <dt className="text-slate-400">Aria</dt>
              <dd className="font-mono text-slate-300">
                {aria.map(([name, value]) => `${name}=${value}`).join(" ")}
              </dd>
            </>
          ) : null}

          <dt className="text-slate-400">Retângulo</dt>
          <dd className="font-mono text-slate-300">
            {`${pick.rect.width}×${pick.rect.height} em (${pick.rect.x}, ${pick.rect.y})`}
          </dd>

          <dt className="text-slate-400">Página</dt>
          <dd className="font-mono text-slate-300">
            {`${pick.viewport.width}×${pick.viewport.height}, rolagem (${pick.scroll.x}, ${pick.scroll.y})`}
          </dd>

          <dt className="text-slate-400">URL</dt>
          <dd className="truncate font-mono text-slate-300" title={pick.url}>
            {pick.url}
          </dd>
        </dl>

        {pick.html ? (
          <pre className="max-h-32 overflow-auto rounded-lg border border-line bg-black/30 p-3 font-mono text-xs whitespace-pre-wrap text-slate-300">
            {pick.html}
          </pre>
        ) : null}

        <label className="flex flex-col gap-1 text-sm text-slate-300">
          O que você quer que mude?
          <textarea
            value={request}
            onChange={(event) => setRequest(event.target.value)}
            rows={4}
            placeholder="Ex.: deixa esse botão verde e menor."
            className="rounded-lg border border-line bg-black/30 px-3 py-2 text-white outline-none focus:border-violet-400/60"
          />
        </label>

        <div className="flex items-center justify-end gap-2">
          <span className="mr-auto text-xs text-slate-500">
            O envio ao agente (sessão ou quadro) chega na F12.4.
          </span>
          <button
            type="button"
            onClick={close}
            className="rounded-lg border border-line px-4 py-2 text-sm text-slate-300 hover:bg-white/10"
          >
            Cancelar
          </button>
          <button
            type="button"
            disabled
            className="rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium text-white hover:bg-violet-500 disabled:opacity-40"
          >
            Enviar
          </button>
        </div>
      </div>
    </div>
  );
}
