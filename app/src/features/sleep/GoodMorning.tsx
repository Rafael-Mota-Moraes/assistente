import { Check, GitMerge, RefreshCw, Sunrise, X } from "lucide-react";
import { useAppStore } from "../../store";
import { closeMorning, mergeMorning, refreshMorning, setMorningDate } from "./morning";

/** Tela "Bom dia" (F6.6): revisar o que o agente fez à noite e mesclar com um clique. */
export function GoodMorning() {
  const morning = useAppStore((s) => s.morning);
  if (!morning) return null;
  const { report, error, busy, merged } = morning;

  return (
    <div
      role="dialog"
      aria-label="Bom dia"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={(event) => {
        if (event.target === event.currentTarget) closeMorning();
      }}
      onKeyDown={() => {}}
    >
      <div className="flex max-h-full w-full max-w-2xl flex-col gap-4 overflow-y-auto rounded-2xl border border-line bg-surface-strong p-6 shadow-2xl">
        <header className="flex items-center gap-3">
          <Sunrise className="size-5 shrink-0 text-amber-300" />
          <h2 className="text-lg font-semibold text-white">
            Bom dia — {morning.project}
            <span className="ml-2 text-sm font-normal text-slate-400">noite de {morning.date}</span>
          </h2>
          <button
            type="button"
            title="Fechar"
            onClick={closeMorning}
            className="ml-auto rounded-md p-1.5 text-slate-400 hover:bg-white/10 hover:text-white"
          >
            <X className="size-4" />
          </button>
        </header>

        <div className="flex items-center gap-2 text-sm">
          <label className="text-slate-400" htmlFor="morning-date">
            Noite
          </label>
          <input
            id="morning-date"
            type="date"
            value={morning.date}
            onChange={(event) => setMorningDate(event.target.value)}
            className="rounded-lg border border-line bg-black/30 px-2 py-1 text-slate-200"
          />
          <button
            type="button"
            onClick={refreshMorning}
            disabled={busy}
            className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-slate-200 hover:bg-white/10 disabled:opacity-50"
          >
            <RefreshCw className="size-3.5" /> Recolher
          </button>
        </div>

        {error && (
          <p className="rounded-lg bg-rose-950/50 px-3 py-2 font-mono text-xs text-rose-300">
            {error}
          </p>
        )}

        {busy && !report && (
          <p className="text-sm text-slate-400">Buscando o resultado da noite no notebook…</p>
        )}

        {report && (
          <>
            <section aria-label="O que foi feito">
              <h3 className="text-sm font-semibold text-slate-300">
                O que foi feito{" "}
                <span className="font-normal text-slate-500">
                  ({report.commits.length} commit{report.commits.length === 1 ? "" : "s"} em{" "}
                  {report.sleep_branch} → {report.base})
                </span>
              </h3>
              {report.commits.length === 0 ? (
                <p className="mt-1 text-sm text-slate-500">Nenhum commit novo nesta noite.</p>
              ) : (
                <ul className="mt-2 flex flex-col gap-1">
                  {report.commits.map((commit) => (
                    <li key={commit.hash} className="font-mono text-xs text-slate-300">
                      <span className="text-slate-500">{commit.hash.slice(0, 7)}</span>{" "}
                      {commit.subject}
                    </li>
                  ))}
                </ul>
              )}
            </section>

            {report.relatorio && (
              <section aria-label="Relatório do agente">
                <h3 className="text-sm font-semibold text-amber-300">RELATORIO.md</h3>
                <pre className="mt-1 max-h-48 overflow-y-auto whitespace-pre-wrap rounded-lg bg-black/40 p-3 font-mono text-xs text-amber-100/90">
                  {report.relatorio}
                </pre>
              </section>
            )}

            {merged ? (
              <p className="flex items-center gap-2 rounded-lg bg-emerald-950/60 px-3 py-2 text-sm text-emerald-300">
                <Check className="size-4" /> Mesclado em {report.base} (fast-forward).
              </p>
            ) : report.fast_forward && report.commits.length > 0 ? (
              <button
                type="button"
                onClick={mergeMorning}
                disabled={busy}
                className="flex w-fit items-center gap-2 rounded-lg bg-gradient-to-r from-violet-600 to-sky-600 px-4 py-2 text-sm font-semibold text-white shadow-lg shadow-violet-900/30 disabled:opacity-60"
              >
                <GitMerge className="size-4" /> Mesclar em {report.base}
              </button>
            ) : report.fast_forward ? null : (
              <p className="rounded-lg bg-amber-950/60 px-3 py-2 text-sm text-amber-200">
                A base andou durante a noite: não dá para mesclar com um clique (só fast-forward).
                Mescle à mão.
              </p>
            )}
          </>
        )}
      </div>
    </div>
  );
}
