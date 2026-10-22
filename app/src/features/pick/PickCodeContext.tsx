import { FileCode2, Search } from "lucide-react";
import { describeComponents, resolveCodeContext, searchInstruction } from "./context";
import type { PickElement } from "./types";

/** Bloco "onde está" do elemento apontado (F12.3).
 *
 * Em dev (Vite + React) mostra o arquivo e a linha do JSX; sem fonte (build de produção)
 * mostra o componente, o seletor e o texto, e avisa que o agente vai procurar o arquivo
 * antes de editar. */
export function PickCodeContext({
  pick,
  projectRoot,
}: {
  pick: PickElement;
  projectRoot?: string | null;
}) {
  const context = resolveCodeContext(pick.code, projectRoot);
  const components = describeComponents(context);
  return (
    <section
      aria-label="Onde está o elemento"
      className="flex flex-col gap-1 rounded-lg border border-line bg-black/30 px-3 py-2 text-xs"
    >
      {context.source ? (
        <p className="flex items-center gap-1.5 font-medium text-violet-200">
          <FileCode2 className="size-3.5 shrink-0" />
          <span className="font-mono break-all">{context.source.label}</span>
        </p>
      ) : (
        <p className="flex items-center gap-1.5 font-medium text-amber-200">
          <Search className="size-3.5 shrink-0" /> Sem arquivo de origem
        </p>
      )}
      {components ? <p className="text-slate-400">{components}</p> : null}
      {!context.source ? (
        <p className="text-slate-400">
          seletor {pick.selector}
          {pick.text ? ` · texto "${pick.text}"` : ""}
        </p>
      ) : null}
      {!context.source ? (
        <p className="text-amber-300/90">{searchInstruction(pick, context)}</p>
      ) : null}
    </section>
  );
}
