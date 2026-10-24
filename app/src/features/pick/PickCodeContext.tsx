import { FileCode2, Search } from "lucide-react";
import type { PickDetail } from "../../bridge/types";

/** Bloco "onde está" do elemento apontado (F12.3), na caixa de pedido (F12.4).
 *
 * Em dev (Vite + React) mostra o arquivo e a linha do JSX; sem fonte (build de produção)
 * mostra o componente, o seletor e o texto, e avisa que o agente vai procurar o arquivo
 * antes de editar (o aviso em si vai no prompt do hubd). */
export function PickCodeContext({ pick }: { pick: PickDetail }) {
  const components = [
    pick.component ? `componente ${pick.component}` : null,
    pick.components && pick.components.length > 1 ? `pilha: ${pick.components.join(" > ")}` : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <section
      aria-label="Onde está o elemento"
      className="flex flex-col gap-1 rounded-lg border border-line bg-black/30 px-3 py-2 text-xs"
    >
      {pick.source ? (
        <p className="flex items-center gap-1.5 font-medium text-violet-200">
          <FileCode2 className="size-3.5 shrink-0" />
          <span className="font-mono break-all">{pick.source}</span>
        </p>
      ) : (
        <p className="flex items-center gap-1.5 font-medium text-amber-200">
          <Search className="size-3.5 shrink-0" /> Sem arquivo de origem
        </p>
      )}
      {components ? <p className="text-slate-400">{components}</p> : null}
      {!pick.source ? (
        <>
          <p className="text-slate-400">
            seletor {pick.selector}
            {pick.text ? ` · texto "${pick.text}"` : ""}
          </p>
          <p className="text-amber-300/90">
            O agente vai procurar o arquivo (texto, classe, componente) antes de editar.
          </p>
        </>
      ) : null}
    </section>
  );
}
