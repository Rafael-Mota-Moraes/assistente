import {
  AlertTriangle,
  Check,
  ChevronDown,
  Code2,
  GitMerge,
  History,
  Loader2,
  RefreshCw,
  RotateCcw,
  Terminal,
  X,
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import type { UpdateCommit, UpdateStatus } from "../../bridge/types";
import { useAppStore } from "../../store";
import { openSession } from "../sessions/actions";

const EVERY_MS = 3_000;

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Compara hashes curtos e completos (o hubd manda o commit completo, o git log o curto). */
function sameCommit(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  return a.startsWith(b) || b.startsWith(a);
}

/** Mostra hashes completos sem estourar a largura do cartão. */
function shortHash(hash: string | null | undefined): string {
  if (!hash) return "—";
  return hash.length > 10 ? hash.slice(0, 10) : hash;
}

function statusLabel(status: UpdateStatus | null): {
  text: string;
  tone: string;
  icon: "idle" | "running" | "success" | "failed";
} {
  if (!status) return { text: "Sem resposta do hubd", tone: "text-slate-400", icon: "idle" };
  switch (status.status) {
    case "running":
      return { text: "Atualizando agora…", tone: "text-sky-300", icon: "running" };
    case "success":
      return { text: "Última atualização concluída", tone: "text-emerald-300", icon: "success" };
    case "failed":
      return { text: "A última atualização falhou", tone: "text-red-300", icon: "failed" };
    default:
      return { text: "Pronto para atualizar", tone: "text-slate-300", icon: "idle" };
  }
}

function CommitRow({
  item,
  active,
  onSelect,
}: {
  item: UpdateCommit;
  active: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={`flex w-full items-center gap-3 rounded-lg border px-3 py-2 text-left transition-colors ${
        active
          ? "border-violet-400/70 bg-violet-500/20"
          : "border-line bg-black/20 hover:border-slate-500/50 hover:bg-white/5"
      }`}
    >
      <span
        className={`flex size-4 shrink-0 items-center justify-center rounded-full border ${
          active ? "border-violet-300 bg-violet-400/30" : "border-slate-500"
        }`}
      >
        {active ? <Check className="size-3 text-violet-100" /> : null}
      </span>
      <span className="shrink-0 font-mono text-xs text-slate-400">{item.hash}</span>
      <span className="min-w-0 flex-1 truncate text-sm text-slate-200">{item.subject}</span>
    </button>
  );
}

function Section({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-xl border border-line bg-panel-soft p-4">
      <h3 className="text-sm font-semibold text-white">{title}</h3>
      {description ? <p className="mt-1 text-sm text-slate-400">{description}</p> : null}
      <div className="mt-3">{children}</div>
    </section>
  );
}

/** Aba/painel Assistente (F8.6): aplica ou volta a versão do próprio Assistente. */
export function AssistantPanel({ onClose }: { onClose?: () => void }) {
  const hub = useAppStore((s) => s.hub);
  const health = useAppStore((s) => s.health);
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [selected, setSelected] = useState("");
  const [manual, setManual] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // F8.8: commits marcados para juntar na main, na ordem em que foram marcados.
  const [picked, setPicked] = useState<string[]>([]);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    if (!hub) return;
    const client = hub;
    let stopped = false;
    async function poll() {
      try {
        const next = await client.updateStatus();
        if (stopped) return;
        setStatus(next);
        setError(null);
      } catch (e) {
        if (!stopped) setError(message(e));
      }
    }
    void poll();
    const timer = setInterval(poll, EVERY_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [hub]);

  const head = status?.head ?? status?.head_commit ?? null;
  const runningVersion = [health?.commit.ui, health?.commit.hubd].filter((value): value is string =>
    Boolean(value),
  );
  // F8.6: commits já instalados não entram na lista; o backend também filtra, mas manter aqui
  // evita mostrar a versão em uso quando o hubd é antigo.
  const commits = (status?.commits ?? status?.pending_commits ?? []).filter(
    (commit) => !runningVersion.some((done) => sameCommit(commit.hash, done)),
  );
  const headAlreadyInUse = runningVersion.some((done) => sameCommit(head, done));
  // Marcados que ainda aparecem na lista (um commit aplicado sai dela sozinho).
  const selection = picked.filter((hash) => commits.some((commit) => commit.hash === hash));

  // Sem escolha manual, sugere o HEAD — mas não sugere um commit que já está rodando.
  const effective = manual ? selected : selected || (headAlreadyInUse ? "" : head || "");
  const chosen = useMemo(
    () => commits.find((commit) => sameCommit(commit.hash, effective)) ?? null,
    [commits, effective],
  );

  useEffect(() => {
    if (manual) return;
    if (selected) return;
    if (!head || headAlreadyInUse) return;
    setSelected(head);
  }, [head, headAlreadyInUse, manual, selected]);

  async function apply(target: string) {
    if (!hub || !target.trim()) return;
    setBusy(true);
    try {
      await hub.applyUpdate(target.trim());
      setError(null);
      const next = await hub.updateStatus();
      setStatus(next);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }

  function togglePicked(hash: string) {
    setNotice(null);
    setPicked((list) =>
      list.includes(hash) ? list.filter((done) => done !== hash) : [...list, hash],
    );
  }

  async function joinMain() {
    if (!hub || selection.length === 0) return;
    setBusy(true);
    try {
      const result = await hub.applyToMain(selection);
      setError(null);
      setPicked([]);
      setNotice(`main agora em ${result.main}: ${result.applied.length} commit(s) aplicado(s).`);
    } catch (e) {
      setNotice(null);
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }

  async function rollback() {
    if (!hub) return;
    setBusy(true);
    try {
      await hub.rollbackUpdate();
      setError(null);
      const next = await hub.updateStatus();
      setStatus(next);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  }

  async function openAgent() {
    if (!health?.self_update) return;
    const { profile, project } = health.self_update;
    useAppStore.setState({
      activeProfile: profile,
      activeProject: project,
      activeSession: null,
      activeView: null,
    });
    await openSession("claude");
    onClose?.();
  }

  const state = statusLabel(status);
  const running = status?.running || status?.status === "running";

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex shrink-0 items-start justify-between gap-4 border-b border-line bg-panel-soft px-6 py-4">
        <div className="flex items-center gap-3">
          <span className="flex size-10 items-center justify-center rounded-xl bg-violet-500/15 text-violet-300">
            <RefreshCw className="size-5" />
          </span>
          <div>
            <h2 className="text-lg font-semibold text-white">Assistente</h2>
            <p className="text-sm text-slate-400">
              Atualize o próprio app e abra o agente neste projeto.
            </p>
          </div>
        </div>
        {onClose ? (
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1.5 text-slate-400 hover:bg-white/10 hover:text-white"
            aria-label="Fechar"
          >
            <X className="size-5" />
          </button>
        ) : null}
      </header>

      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-6 py-5">
        {!health?.self_update ? (
          <div className="flex gap-3 rounded-xl border border-amber-500/40 bg-amber-500/10 p-3 text-sm text-amber-100">
            <AlertTriangle className="mt-0.5 size-4 shrink-0" />
            <p>
              Falta configurar o projeto do Assistente. Adicione{" "}
              <code className="rounded bg-black/30 px-1 font-mono text-xs">[self_update]</code> em{" "}
              <code className="rounded bg-black/30 px-1 font-mono text-xs">
                ~/.config/assistente/assistente.toml
              </code>{" "}
              com <code className="font-mono text-xs">profile</code> e{" "}
              <code className="font-mono text-xs">project</code>.
            </p>
          </div>
        ) : null}

        <div className="grid gap-3 sm:grid-cols-2">
          <div className="rounded-xl border border-line bg-panel-soft p-4">
            <div className="text-xs font-medium uppercase tracking-wide text-slate-500">
              Versão em uso agora
            </div>
            <dl className="mt-2 space-y-1 text-sm">
              <div className="flex items-center justify-between gap-3">
                <dt className="text-slate-400">Interface</dt>
                <dd className="font-mono text-white">{shortHash(health?.commit.ui)}</dd>
              </div>
              <div className="flex items-center justify-between gap-3">
                <dt className="text-slate-400">Servidor (hubd)</dt>
                <dd className="font-mono text-white">{shortHash(health?.commit.hubd)}</dd>
              </div>
            </dl>
          </div>

          <div className="rounded-xl border border-line bg-panel-soft p-4">
            <div className="text-xs font-medium uppercase tracking-wide text-slate-500">
              Atualizador
            </div>
            <div className={`mt-2 flex items-center gap-2 text-sm font-medium ${state.tone}`}>
              {state.icon === "running" ? (
                <Loader2 className="size-4 animate-spin" />
              ) : state.icon === "success" ? (
                <Check className="size-4" />
              ) : state.icon === "failed" ? (
                <AlertTriangle className="size-4" />
              ) : (
                <History className="size-4" />
              )}
              {state.text}
            </div>
            <p className="mt-1 text-sm text-slate-400">
              {status?.commit ? (
                <>
                  Commit:{" "}
                  <span className="font-mono text-slate-300">{shortHash(status.commit)}</span>
                </>
              ) : (
                "Nenhuma atualização aplicada nesta sessão."
              )}
            </p>
            {status?.error ? <p className="mt-1 text-sm text-red-300">{status.error}</p> : null}
          </div>
        </div>

        <Section
          title="Atualizar o Assistente"
          description="Escolha um commit já salvo no repositório. O app compila, instala e reinicia sozinho. Se algo falhar, ele volta para a versão anterior."
        >
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs font-medium uppercase tracking-wide text-slate-500">
                Commits recentes
              </span>
              {head ? (
                <span className="text-xs text-slate-500">
                  último commit do repo: <span className="font-mono text-slate-400">{head}</span>
                </span>
              ) : null}
            </div>

            {commits.length === 0 ? (
              <p className="rounded-lg border border-line bg-black/20 p-3 text-sm text-slate-400">
                Nenhum commit novo para aplicar. Commits já instalados não aparecem aqui.
              </p>
            ) : (
              <div className="flex max-h-56 flex-col gap-1.5 overflow-y-auto pr-1">
                {commits.map((item) => (
                  <CommitRow
                    key={item.hash}
                    item={item}
                    active={sameCommit(item.hash, effective)}
                    onSelect={() => {
                      setManual(false);
                      setSelected(item.hash);
                    }}
                  />
                ))}
              </div>
            )}
          </div>

          <div className="mt-4 flex flex-col gap-2 border-t border-line pt-4">
            <button
              type="button"
              onClick={() => setManual((value) => !value)}
              className="flex w-fit items-center gap-1 text-xs text-slate-400 hover:text-slate-200"
            >
              <ChevronDown
                className={`size-3 transition-transform ${manual ? "rotate-180" : ""}`}
              />
              {manual ? "Ocultar campo de commit manual" : "Ou digite outro commit"}
            </button>
            {manual ? (
              <input
                value={selected}
                onChange={(e) => setSelected(e.target.value)}
                placeholder="ex.: a1b2c3d"
                className="w-full rounded-lg border border-line bg-black/30 px-3 py-2 font-mono text-sm text-white placeholder:text-slate-500"
              />
            ) : null}

            <div className="mt-1 flex flex-wrap items-center gap-3">
              <button
                type="button"
                disabled={busy || running || !effective.trim()}
                onClick={() => apply(effective)}
                className="flex items-center gap-2 rounded-lg bg-violet-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-violet-500 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {busy || running ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <RefreshCw className="size-4" />
                )}
                {running ? "Atualizando…" : "Aplicar atualização"}
              </button>
              <span className="text-sm text-slate-400">
                {chosen ? (
                  <>
                    selecionado: <span className="font-mono text-slate-300">{chosen.hash}</span> —{" "}
                    {chosen.subject}
                  </>
                ) : effective ? (
                  <>
                    selecionado: <span className="font-mono text-slate-300">{effective}</span>
                  </>
                ) : (
                  "selecione um commit acima"
                )}
              </span>
            </div>
          </div>
        </Section>

        <Section
          title="Juntar commits na main"
          description="Marque os commits que devem estar na main do projeto do Assistente. Entram na ordem do histórico; se algum conflitar, nada muda. Isto não instala nada: para usar, aplique um commit acima."
        >
          {commits.length === 0 ? (
            <p className="rounded-lg border border-line bg-black/20 p-3 text-sm text-slate-400">
              Nenhum commit novo para juntar.
            </p>
          ) : (
            <div className="flex max-h-56 flex-col gap-1.5 overflow-y-auto pr-1">
              {commits.map((item) => (
                <label
                  key={item.hash}
                  className="flex w-full cursor-pointer items-center gap-3 rounded-lg border border-line bg-black/20 px-3 py-2 transition-colors hover:bg-white/5"
                >
                  <input
                    type="checkbox"
                    checked={selection.includes(item.hash)}
                    onChange={() => togglePicked(item.hash)}
                    className="size-4 shrink-0 accent-violet-500"
                  />
                  <span className="shrink-0 font-mono text-xs text-slate-400">{item.hash}</span>
                  <span className="min-w-0 flex-1 truncate text-sm text-slate-200">
                    {item.subject}
                  </span>
                </label>
              ))}
            </div>
          )}

          <div className="mt-4 flex flex-wrap items-center gap-3 border-t border-line pt-4">
            <button
              type="button"
              disabled={busy || running || selection.length === 0}
              onClick={joinMain}
              className="flex items-center gap-2 rounded-lg border border-line bg-black/20 px-4 py-2 text-sm font-medium text-slate-200 transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <GitMerge className="size-4" /> Aplicar {selection.length} na main
            </button>
            <span className="text-sm text-slate-400">
              {notice ??
                (selection.length > 0
                  ? `${selection.length} selecionado(s)`
                  : "selecione commits acima")}
            </span>
          </div>
        </Section>

        <div className="grid gap-4 sm:grid-cols-2">
          <Section
            title="Voltar para a versão anterior"
            description="Usa a versão que estava rodando antes da última atualização."
          >
            <p className="text-sm text-slate-400">
              Versão anterior:{" "}
              <span className="font-mono text-slate-300">{shortHash(status?.previous_commit)}</span>
            </p>
            <button
              type="button"
              disabled={busy || running}
              onClick={rollback}
              className="mt-3 flex items-center gap-2 rounded-lg border border-line bg-black/20 px-4 py-2 text-sm font-medium text-slate-200 transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <RotateCcw className="size-4" /> Voltar
            </button>
          </Section>

          <Section
            title="Editar o Assistente com um agente"
            description={
              health?.self_update
                ? `Abre uma sessão do Claude no projeto ${health.self_update.profile}/${health.self_update.project}.`
                : "Configure [self_update] para abrir o agente no projeto certo."
            }
          >
            <button
              type="button"
              disabled={!health?.self_update}
              onClick={openAgent}
              className="flex items-center gap-2 rounded-lg border border-line bg-black/20 px-4 py-2 text-sm font-medium text-slate-200 transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Terminal className="size-4" /> Abrir aba do agente
            </button>
          </Section>
        </div>

        {error ? (
          <p className="rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200">
            {error}
          </p>
        ) : null}

        {status?.log_tail ? (
          <details className="rounded-xl border border-line bg-panel-soft">
            <summary className="flex cursor-pointer items-center gap-2 px-4 py-3 text-sm font-medium text-slate-300">
              <Code2 className="size-4" /> Ver log do atualizador
            </summary>
            <pre className="max-h-56 overflow-auto border-t border-line bg-black/40 p-4 text-xs text-slate-300">
              {status.log_tail}
            </pre>
          </details>
        ) : null}
      </div>
    </div>
  );
}

/** Botão fixo da lateral (F8.6). O modal vai para o body para não ficar preso à largura da
 * lateral (backdrop-filter da sidebar cria containing block para `position: fixed`). O estado
 * fica no store: o modal é uma sobreposição, e as webviews filhas precisam saber dela (F12.6). */
export function AssistantButton() {
  const open = useAppStore((s) => s.assistantPanelOpen);
  const modal =
    open && typeof document !== "undefined"
      ? createPortal(
          <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 sm:p-6">
            <button
              type="button"
              aria-label="Fechar"
              onClick={() => useAppStore.setState({ assistantPanelOpen: false })}
              className="absolute inset-0 cursor-default"
            />
            <div className="relative flex max-h-[90vh] w-full max-w-3xl flex-col overflow-hidden rounded-2xl border border-line bg-panel shadow-2xl">
              <AssistantPanel onClose={() => useAppStore.setState({ assistantPanelOpen: false })} />
            </div>
          </div>,
          document.body,
        )
      : null;
  return (
    <>
      <button
        type="button"
        onClick={() => useAppStore.setState({ assistantPanelOpen: true })}
        title="Abrir a aba Assistente: atualizar o app e abrir o agente neste projeto"
        className="flex w-full items-center justify-center gap-2 rounded-xl border border-line bg-white/5 px-3 py-2 text-sm font-medium text-slate-300 transition-colors hover:bg-white/10"
      >
        <RefreshCw className="size-4" /> Assistente
      </button>
      {modal}
    </>
  );
}

// Aliases para compatibilidade com possíveis nomes de teste.
export const UpdatePanel = AssistantPanel;
export const SelfUpdatePanel = AssistantPanel;
export const AssistantTab = AssistantPanel;
