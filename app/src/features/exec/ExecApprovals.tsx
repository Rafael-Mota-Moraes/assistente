import { Check, ChevronDown, ChevronUp, ShieldAlert, TriangleAlert, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ExecView } from "../../bridge/types";
import { useAppStore } from "../../store";
import { tabLabel } from "../sessions/SessionArea";
import { approveOnHost, autoRunOnHost } from "./hostExec";

const EVERY_MS = 2000;

/** Pedidos fora do jail mudam rápido (o agente está esperando): poll próprio, mais curto. */
export function useExecPolling() {
  const hub = useAppStore((s) => s.hub);
  const online = useAppStore((s) => s.health !== null);
  useEffect(() => {
    if (!hub || !online) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function tick() {
      try {
        const execRequests = await hub?.execRequests();
        if (!stopped && execRequests) useAppStore.setState({ execRequests });
      } catch {
        // hubd fora: o usePolling já mostra o erro.
      }
      if (!stopped) timer = setTimeout(tick, EVERY_MS);
    }
    tick();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [hub, online]);
}

async function answer(request: ExecView, verb: "approve" | "always" | "deny") {
  const { hub } = useAppStore.getState();
  if (!hub) return;
  try {
    if (verb === "deny") await hub.denyExec(request.id);
    // Pedido para o PC (F9.10): o hubd não executa nada na VM; o app roda no host e
    // devolve o resultado para o agente.
    else if (request.target === "host") await approveOnHost(request, hub);
    else await hub.approveExec(request.id, verb === "always");
    useAppStore.setState({ execRequests: await hub.execRequests() });
  } catch (error) {
    useAppStore.setState({ toast: error instanceof Error ? error.message : String(error) });
  }
}

const ALWAYS_WARNING =
  "Se o comando roda arquivos do projeto (./dev, make, npm run, scripts), o agente pode " +
  "editá-los e rodar qualquer coisa fora do jail sem perguntar.";

/** Painel fixo (F9.7): o agente pediu para rodar algo fora do jail; você decide. */
export function ExecApprovals() {
  useExecPolling();
  const requests = useAppStore((s) => s.execRequests);
  const sessions = useAppStore((s) => s.sessions);
  const [showRecent, setShowRecent] = useState(false);
  // Pedido em andamento (rodando no PC, F9.10): não aprova de novo.
  const [busy, setBusy] = useState<string[]>([]);

  // Pedido para o PC já passado pela lista do projeto (F9.11): uma vez por id.
  const checked = useRef(new Set<string>());

  const pending = requests.filter((r) => r.state === "pending");
  const recent = requests.filter((r) => r.state !== "pending").slice(0, 8);

  // O app confere no host se o comando está, texto exato, na lista do PC do projeto; se
  // estiver, aprova e roda sem clique. Fora da lista o card espera você.
  useEffect(() => {
    const { hub } = useAppStore.getState();
    if (!hub) return;
    for (const request of requests) {
      if (request.target !== "host" || request.state !== "pending") continue;
      if (checked.current.has(request.id)) continue;
      checked.current.add(request.id);
      setBusy((ids) => [...ids, request.id]);
      autoRunOnHost(request, hub)
        .then(async (ran) => {
          if (ran) useAppStore.setState({ execRequests: await hub.execRequests() });
        })
        .catch((error: unknown) => {
          useAppStore.setState({ toast: error instanceof Error ? error.message : String(error) });
        })
        .finally(() => setBusy((ids) => ids.filter((id) => id !== request.id)));
    }
  }, [requests]);

  if (pending.length === 0 && recent.length === 0) return null;

  const origin = (request: ExecView) => {
    const session = sessions.find((s) => s.id === request.session);
    return `${request.project} · ${session ? tabLabel(session) : request.session}`;
  };

  const respond = (request: ExecView, verb: "approve" | "always" | "deny") => {
    if (busy.includes(request.id)) return;
    setBusy((ids) => [...ids, request.id]);
    answer(request, verb).finally(() => {
      setBusy((ids) => ids.filter((id) => id !== request.id));
    });
  };

  return (
    <aside
      aria-label="Pedidos fora do jail"
      className="fixed right-4 bottom-12 z-40 flex max-h-[70vh] w-[30rem] max-w-[calc(100vw-2rem)] flex-col gap-2 overflow-y-auto"
    >
      {pending.map((request) => {
        const isHost = request.target === "host";
        return (
          <section
            key={request.id}
            aria-label={`Pedido de ${origin(request)}`}
            className={`flex flex-col gap-2 rounded-xl border p-3 shadow-2xl backdrop-blur ${
              isHost ? "border-red-500/70 bg-red-950/90" : "border-amber-500/40 bg-surface-strong"
            }`}
          >
            <header className="flex items-center gap-2 text-sm">
              {isHost ? (
                <TriangleAlert className="size-4 shrink-0 text-red-300" />
              ) : (
                <ShieldAlert className="size-4 shrink-0 text-amber-300" />
              )}
              <span className={`font-medium`}>
                <span className={isHost ? "text-red-100" : "text-amber-100"}>
                  {isHost ? "Vai rodar no seu PC" : "Rodar fora do jail?"}
                </span>
              </span>
              <span className="ml-auto text-xs text-slate-400">{origin(request)}</span>
            </header>
            {/* No PC, comando e pasta aparecem por inteiro (F9.11): nada escondido atrás de rolagem. */}
            <pre
              className={`overflow-auto rounded-lg bg-black/40 px-2 py-1.5 font-mono text-xs whitespace-pre-wrap break-all text-slate-100 ${
                isHost ? "" : "max-h-40"
              }`}
            >
              {request.command}
            </pre>
            {isHost ? (
              <p className="font-mono text-[11px] break-all text-red-200">
                em {request.cwd} · até {Math.round(request.timeout_secs / 60)} min · no PC, com as
                suas chaves e logins
              </p>
            ) : (
              <p className="truncate font-mono text-[11px] text-slate-500" title={request.cwd}>
                em {request.cwd} · até {Math.round(request.timeout_secs / 60)} min · na VM
              </p>
            )}
            <div className="flex items-center gap-2">
              <button
                type="button"
                disabled={busy.includes(request.id)}
                onClick={() => respond(request, "approve")}
                className={`flex items-center gap-1 rounded-lg px-3 py-1 text-sm font-medium ${
                  isHost
                    ? "bg-red-600 text-white hover:bg-red-500"
                    : "bg-amber-600 text-white hover:bg-amber-500"
                } disabled:opacity-50`}
              >
                <Check className="size-3.5" /> Aprovar
              </button>
              <button
                type="button"
                disabled={busy.includes(request.id)}
                onClick={() => respond(request, "deny")}
                className="flex items-center gap-1 rounded-lg border border-line bg-white/5 px-3 py-1 text-sm text-slate-200 hover:bg-white/10 disabled:opacity-50"
              >
                <X className="size-3.5" /> Negar
              </button>
              {/* "Sempre permitir" é só da VM: no PC, sem clique só a lista exata da engrenagem (F9.11). */}
              {!isHost && (
                <button
                  type="button"
                  title={ALWAYS_WARNING}
                  onClick={() => {
                    if (
                      window.confirm(
                        `Sempre permitir este comando em ${request.project}?\n\n${request.command}\n\n${ALWAYS_WARNING}`,
                      )
                    ) {
                      respond(request, "always");
                    }
                  }}
                  className="ml-auto text-xs text-slate-400 underline-offset-2 hover:text-slate-200 hover:underline"
                >
                  Aprovar e sempre permitir neste projeto
                </button>
              )}
            </div>
          </section>
        );
      })}

      {recent.length > 0 && (
        <section className="rounded-xl border border-line bg-surface-strong text-sm shadow-xl backdrop-blur">
          <button
            type="button"
            onClick={() => setShowRecent((v) => !v)}
            className="flex w-full items-center gap-2 px-3 py-1.5 text-xs text-slate-400 hover:text-slate-200"
          >
            {showRecent ? <ChevronDown className="size-3.5" /> : <ChevronUp className="size-3.5" />}
            Fora do jail: {recent.length} recente{recent.length === 1 ? "" : "s"}
          </button>
          {showRecent && (
            <ul className="flex flex-col gap-1 px-3 pb-2">
              {recent.map((request) => (
                <li key={request.id} className="flex items-center gap-2 text-xs">
                  <Outcome request={request} />
                  <span className="truncate font-mono text-slate-300" title={request.command}>
                    {request.command}
                  </span>
                  <span className="ml-auto shrink-0 text-slate-500">{origin(request)}</span>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}
    </aside>
  );
}

function Outcome({ request }: { request: ExecView }) {
  const { state, result, auto_approved, target } = request;
  const [label, tone] =
    state === "running"
      ? target === "host"
        ? ["rodando no PC", "text-amber-300"]
        : ["rodando", "text-sky-300"]
      : state === "denied"
        ? ["negado", "text-slate-400"]
        : state === "expired"
          ? ["expirou", "text-slate-500"]
          : state === "failed"
            ? ["falhou", "text-rose-300"]
            : result?.timed_out
              ? ["timeout", "text-rose-300"]
              : result?.exit_code === 0
                ? ["ok", "text-emerald-300"]
                : [`código ${result?.exit_code ?? "?"}`, "text-rose-300"];
  return (
    <span className={`shrink-0 font-medium ${tone}`}>
      {label}
      {auto_approved && <span className="ml-1 text-slate-500">(sempre)</span>}
    </span>
  );
}
