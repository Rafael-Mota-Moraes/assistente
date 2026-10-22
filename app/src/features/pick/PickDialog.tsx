import { ChevronDown, Crosshair, Send, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { PickDetail, PickTarget } from "../../bridge/types";
import { openView, useAppStore, viewKey } from "../../store";
import { reopenTab } from "../sessions/shortcuts";

/** Espera antes de pedir o prompt: cada tecla não vira uma ida ao hubd. */
const PREVIEW_MS = 300;

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Onde o elemento apontado está: o `arquivo:linha` (F12.3), o componente ou o caminho no DOM. */
function place(pick: PickDetail): string | null {
  return pick.source ?? pick.component ?? pick.dom_path ?? null;
}

/** Uma linha que descreve o elemento: seletor e texto. */
function describe(pick: PickDetail): string {
  const parts = [pick.selector, pick.text && `"${pick.text}"`].filter(Boolean);
  return parts.length ? parts.join(" ") : "o elemento apontado";
}

/**
 * Caixa de pedido do apontamento (F12.4): mostra o prompt que o agente vai receber, deixa
 * editar o texto e escolher para onde o pedido vai — a sessão de um agente (o texto é colado
 * no prompt, sem Enter: o Enter é do usuário) ou o quadro (vira card no backlog).
 */
export function PickDialog() {
  const target = useAppStore((s) => s.pick);
  const hub = useAppStore((s) => s.hub);
  const sessions = useAppStore((s) => s.sessions);
  const activeSession = useAppStore((s) => s.activeSession);

  const [request, setRequest] = useState("");
  const [mode, setMode] = useState<PickTarget>("sessao");
  const [session, setSession] = useState("");
  const [prompt, setPrompt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  // O pick que o modal está mostrando: quando muda, o modal começa de novo.
  const shown = useRef<typeof target>(null);

  const profile = target?.profile ?? "";
  const project = target?.project ?? "";

  /** As sessões de agente do projeto: a aba Terminal nunca recebe o pedido. */
  const agents = useMemo(
    () =>
      sessions.filter((s) => s.profile === profile && s.project === project && s.agent !== "shell"),
    [sessions, profile, project],
  );

  // Ao abrir: o texto vazio, o alvo sessão e a aba ativa do projeto.
  useEffect(() => {
    if (!target || shown.current === target) return;
    shown.current = target;
    const preferred = agents.find((s) => s.id === activeSession) ?? agents[0];
    setRequest("");
    setMode("sessao");
    setSession(preferred?.id ?? "");
    setPrompt("");
    setError(null);
    setPreviewError(null);
    // A lista de sessões e a aba ativa só interessam na abertura: o `shown` impede que a
    // lista que muda a cada 5 s apague o texto que o usuário está escrevendo.
  }, [target, agents, activeSession]);

  // O prompt que será enviado: quem monta é o hubd (URL, onde, seletor, texto, pedido).
  useEffect(() => {
    if (!hub || !target) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      hub
        .previewPick(target.profile, target.project, { pick: target.pick, request })
        .then((view) => {
          if (cancelled) return;
          setPrompt(view.prompt);
          setPreviewError(null);
        })
        .catch((e: unknown) => {
          if (!cancelled) setPreviewError(message(e));
        });
    }, PREVIEW_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [hub, target, request]);

  if (!target) return null;

  const pick = target.pick;

  function close() {
    useAppStore.setState({ pick: null });
  }

  async function send() {
    if (!hub || !target) return;
    setSending(true);
    setError(null);
    try {
      const result = await hub.sendPick(target.profile, target.project, {
        target: mode,
        session: mode === "sessao" ? session : null,
        request,
        pick: target.pick,
      });
      if (result.session) {
        // O texto foi colado no prompt: leva para a aba (reabrindo, se estava fechada) e
        // lembra que o Enter é do usuário.
        reopenTab(result.session);
        useAppStore.setState({
          toast: "Pedido colado no prompt da sessão. Confirme no terminal (Enter).",
        });
      } else {
        openView(viewKey(target.profile, target.project), "board");
        useAppStore.setState({
          toast: `Card ${result.card?.id ?? ""} criado no backlog do quadro.`,
        });
      }
      useAppStore.setState({ pick: null });
    } catch (e) {
      setError(message(e));
    } finally {
      setSending(false);
    }
  }

  const canSend = request.trim().length > 0 && (mode === "quadro" || session !== "");

  return (
    <div
      role="dialog"
      aria-label={`Pedido em ${project}`}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={(event) => {
        if (event.target === event.currentTarget) close();
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") close();
      }}
    >
      <div className="flex max-h-full w-full max-w-2xl flex-col gap-4 overflow-y-auto rounded-2xl border border-line bg-surface-strong p-6 shadow-2xl">
        <header className="flex items-center gap-3">
          <Crosshair className="size-5 shrink-0 text-slate-300" />
          <h2 className="text-lg font-semibold text-white">
            {project}
            <span className="ml-2 text-sm font-normal text-slate-400">pedido na tela</span>
          </h2>
          <button
            type="button"
            title="Fechar"
            onClick={close}
            className="ml-auto rounded-md p-1.5 text-slate-400 hover:bg-white/10 hover:text-white"
          >
            <X className="size-4" />
          </button>
        </header>

        <dl className="grid gap-1 rounded-lg border border-line bg-black/20 p-3 text-xs text-slate-300">
          <div className="flex gap-2">
            <dt className="w-20 shrink-0 text-slate-500">Página</dt>
            <dd className="truncate font-mono">{pick.url || "—"}</dd>
          </div>
          <div className="flex gap-2">
            <dt className="w-20 shrink-0 text-slate-500">Elemento</dt>
            <dd className="truncate font-mono">{describe(pick)}</dd>
          </div>
          {place(pick) && (
            <div className="flex gap-2">
              <dt className="w-20 shrink-0 text-slate-500">Onde</dt>
              <dd className="truncate font-mono">{place(pick)}</dd>
            </div>
          )}
        </dl>

        <label className="flex flex-col gap-1 text-sm text-slate-300">
          O que quer mudar
          <textarea
            aria-label="O que quer mudar"
            value={request}
            onChange={(event) => setRequest(event.target.value)}
            rows={3}
            placeholder="deixa verde"
            className="rounded-lg border border-line bg-black/30 px-3 py-2 text-white outline-none focus:border-accent/60"
          />
        </label>

        <section className="flex flex-col gap-2 text-sm text-slate-300">
          <h3 className="text-sm font-medium text-slate-200">O que o agente vai receber</h3>
          <pre className="max-h-32 overflow-y-auto whitespace-pre-wrap rounded-lg border border-line bg-black/30 p-3 font-mono text-xs text-slate-200">
            {prompt || "…"}
          </pre>
          {previewError && (
            <p className="font-mono text-xs text-amber-300">
              Não consegui montar o prompt: {previewError}
            </p>
          )}
        </section>

        <fieldset className="flex flex-col gap-2 text-sm text-slate-300">
          <legend className="mb-1">Para onde vai</legend>
          <label
            className={`flex cursor-pointer items-start gap-3 rounded-lg border px-3 py-2 ${
              mode === "sessao" ? "border-accent/60 bg-accent/10" : "border-line bg-black/20"
            }`}
          >
            <input
              type="radio"
              name="alvo"
              value="sessao"
              checked={mode === "sessao"}
              onChange={() => setMode("sessao")}
              className="mt-1"
            />
            <span className="flex flex-col">
              <span className="font-medium text-white">Sessão</span>
              <span className="text-xs text-slate-400">
                Cola no prompt do agente, sem Enter: você confirma no terminal.
              </span>
            </span>
          </label>
          <label
            className={`flex cursor-pointer items-start gap-3 rounded-lg border px-3 py-2 ${
              mode === "quadro" ? "border-accent/60 bg-accent/10" : "border-line bg-black/20"
            }`}
          >
            <input
              type="radio"
              name="alvo"
              value="quadro"
              checked={mode === "quadro"}
              onChange={() => setMode("quadro")}
              className="mt-1"
            />
            <span className="flex flex-col">
              <span className="font-medium text-white">Quadro</span>
              <span className="text-xs text-slate-400">
                Vira card no backlog: o agente pega depois, com o texto inteiro.
              </span>
            </span>
          </label>
        </fieldset>

        {mode === "sessao" && (
          <label className="flex flex-col gap-1 text-sm text-slate-300">
            Sessão
            <span className="relative">
              {/* Sem aparência nativa: a lista e a seta seguem o tema escuro do app. */}
              <select
                aria-label="Sessão que recebe o pedido"
                value={session}
                onChange={(event) => setSession(event.target.value)}
                className="w-full scheme-dark appearance-none rounded-lg border border-line bg-black/30 py-2 pr-9 pl-3 text-white outline-none focus:border-accent/60 [&>option]:bg-panel"
              >
                {agents.length === 0 && (
                  <option value="">Nenhuma sessão de agente neste projeto</option>
                )}
                {agents.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.id}
                  </option>
                ))}
              </select>
              <ChevronDown className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-slate-400" />
            </span>
          </label>
        )}

        {error && (
          <p className="rounded-lg bg-rose-950/50 px-3 py-2 font-mono text-xs text-rose-300">
            {error}
          </p>
        )}

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={close}
            className="rounded-lg px-3 py-1.5 text-sm text-slate-300 hover:bg-white/10"
          >
            Cancelar
          </button>
          <button
            type="button"
            onClick={() => void send()}
            disabled={!canSend || sending}
            className="flex items-center gap-1.5 rounded-lg bg-violet-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-violet-500 disabled:opacity-50"
          >
            <Send className="size-4" /> {sending ? "Enviando…" : "Enviar"}
          </button>
        </div>
      </div>
    </div>
  );
}
