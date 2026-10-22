import {
  Bot,
  ChevronDown,
  CirclePause,
  LayoutGrid,
  Pencil,
  Play,
  Plus,
  RotateCcw,
  ScrollText,
  ShieldCheck,
  Sparkles,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { type DragEvent, useCallback, useEffect, useRef, useState } from "react";
import type {
  KanbanBoard as Board,
  KanbanCard,
  KanbanColumn,
  KanbanDraft,
  KanbanOrchestrator,
  KanbanPriority,
  SessionInfo,
} from "../../bridge/types";
import { useAppStore, viewKey } from "../../store";
import { claudeLoginCommand } from "../sessions/actions";
import { tabLabel } from "../sessions/SessionArea";
import { TerminalView } from "../sessions/TerminalView";
import { CardDialog } from "./CardDialog";
import { OrchestratorDialog } from "./OrchestratorDialog";

/** Quadro é lido de novo a cada 2 s (F11.3): o que os agentes puxam aparece sem recarregar. */
const EVERY_MS = 2000;
/** Limite de trabalhadores do orquestrador automático (hub_core::kanban::MAX_WORKERS). */
const MAX_WORKERS = 4;

const columns: { id: KanbanColumn; label: string }[] = [
  { id: "backlog", label: "Backlog" },
  { id: "fazendo", label: "Fazendo" },
  { id: "feito", label: "Feito" },
  { id: "validado", label: "Validado" },
];

const priorityStyle: Record<KanbanPriority, string> = {
  alta: "border-rose-400/50 text-rose-200",
  normal: "border-line text-slate-300",
  baixa: "border-sky-400/40 text-sky-200",
};

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Aviso depois de mexer no orquestrador: o que mudou e o que a primeira volta fez. */
export function orchestratorNotice(before: KanbanOrchestrator, after: Board): string {
  const now = after.orchestrator ?? { enabled: false, max_workers: 1 };
  const started = after.started ?? [];
  const opened = started.length
    ? `${started.join(", ")} ${started.length === 1 ? "foi" : "foram"} para trabalhador.`
    : after.orchestrator_error
      ? `Não abriu nada: ${after.orchestrator_error}`
      : "Nenhum card de agente livre no backlog agora; ele confere de novo a cada 20 s.";
  if (before.enabled && !now.enabled) {
    return "Orquestrador desligado: nada novo abre; os trabalhadores em andamento continuam.";
  }
  if (!before.enabled && now.enabled) {
    return `Orquestrador ligado (até ${now.max_workers} em fazendo). ${opened}`;
  }
  if (before.max_workers !== now.max_workers) {
    const limit = `Limite: até ${now.max_workers} em fazendo.`;
    return now.enabled ? `${limit} ${opened}` : `${limit} Vale quando o orquestrador ligar.`;
  }
  return now.instructions
    ? "Instruções salvas: valem para os próximos trabalhadores deste projeto."
    : "Sem instruções extras: os trabalhadores usam só as regras padrão.";
}

/** Hora (HH:MM) de um instante em segundos Unix, para a faixa da pausa. */
function clock(seconds: number): string {
  return new Date(seconds * 1000).toLocaleTimeString("pt-BR", {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Modelo sugerido para terminar uma tarefa parada por cota com o opencode (F11.14). */
const OTHER_MODEL = "9router/free";

/** Quem segura a tarefa: a aba da sessão, ou o id cru se a aba já não está aberta. */
function holderLabel(holder: string, sessions: SessionInfo[]): string {
  const session = sessions.find((s) => s.id === holder);
  return session ? tabLabel(session) : holder;
}

/** Quadro do projeto ativo (F11.3): backlog · fazendo · feito · validado, no lugar da área de
 * sessões. */
export function KanbanBoard() {
  const hub = useAppStore((s) => s.hub);
  const activeProfile = useAppStore((s) => s.activeProfile);
  const activeProject = useAppStore((s) => s.activeProject);
  const sessions = useAppStore((s) => s.sessions);
  const profiles = useAppStore((s) => s.profiles);
  const boardChat = useAppStore((s) => s.boardChat);
  const [board, setBoard] = useState<Board | null>(null);
  // Erro da leitura (poll) e erro da última ação ficam separados: o poll a cada 2 s não pode
  // apagar a mensagem de um "Mandar" que falhou.
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const error = actionError ?? loadError;
  const [editing, setEditing] = useState<KanbanCard | "novo" | null>(null);
  const [dragging, setDragging] = useState<KanbanCard | null>(null);
  const [overColumn, setOverColumn] = useState<KanbanColumn | null>(null);
  const [instructionsOpen, setInstructionsOpen] = useState(false);
  // Enquanto o PUT roda (ligar já abre os primeiros trabalhadores), o botão diz o que faz.
  const [switching, setSwitching] = useState<"Ligando…" | "Desligando…" | null>(null);
  // "Terminar com… outro modelo" (F11.14): o card que mostra o campo do modelo, e o modelo.
  const [otherModel, setOtherModel] = useState<{ id: string; model: string } | null>(null);
  const pausedAt = board?.orchestrator?.paused?.at;
  // `null` = quadro ainda não lido: abrir um quadro já pausado mostra só a faixa.
  const seenPause = useRef<number | undefined | null>(null);
  const loaded = board !== null;

  // Pausa nova (a cota acabou com o quadro aberto): avisa uma vez, além da faixa.
  useEffect(() => {
    if (!loaded) return;
    if (pausedAt !== undefined && seenPause.current === undefined) {
      useAppStore.setState({
        toast: "Cota do provedor esgotada: o orquestrador pausou. Veja o quadro.",
      });
    }
    seenPause.current = pausedAt;
  }, [pausedAt, loaded]);

  const refresh = useCallback(async () => {
    if (!hub || !activeProfile || !activeProject) return;
    try {
      setBoard(await hub.kanban(activeProfile, activeProject));
      setLoadError(null);
    } catch (e) {
      setLoadError(message(e));
    }
  }, [hub, activeProfile, activeProject]);

  useEffect(() => {
    setBoard(null);
    seenPause.current = null;
    void refresh();
    const timer = setInterval(() => void refresh(), EVERY_MS);
    return () => clearInterval(timer);
  }, [refresh]);

  if (!hub || !activeProfile || !activeProject) return null;
  const client = hub;
  const profile = activeProfile;
  const project = activeProject;
  const chat = boardChat?.key === viewKey(profile, project) ? boardChat : null;

  /** Roda uma ação na API, mostra o erro se falhar e atualiza o quadro. Enquanto roda, `busy`. */
  async function act(action: () => Promise<unknown>) {
    setBusy(true);
    setActionError(null);
    try {
      await action();
    } catch (e) {
      setActionError(message(e));
    } finally {
      setBusy(false);
    }
    await refresh();
  }

  function moveTo(card: KanbanCard, column: KanbanColumn) {
    if (card.column === column) return;
    void act(() => client.updateCard(profile, project, card.id, { column }));
  }

  /** Mesma guarda do openSession: sem login o Claude morre dentro do jail e a aba some sem
   * explicação. `retry` diz onde clicar de novo depois do login. */
  function requireClaudeLogin(retry: string) {
    const profileView = profiles.find((p) => p.id === profile);
    if (profileView?.claude_login === false) {
      throw new Error(
        `O Claude ainda não tem login no perfil ${profileView.label}. No PC, rode: ` +
          `${claudeLoginCommand(profile)} — depois clique em ${retry} de novo.`,
      );
    }
  }

  /** "Terminar com… Claude Code" (F11.14): o Claude abre numa aba, no worktree da tarefa. */
  function finishWithClaude(card: KanbanCard) {
    void act(async () => {
      requireClaudeLogin("Terminar com Claude");
      const info = await client.finishWithClaude(profile, project, card.id);
      useAppStore.setState((s) => ({
        sessions: [...s.sessions.filter((x) => x.id !== info.id), info],
        toast: `${card.id} com o Claude, no worktree dela: ele marca o card ao terminar.`,
      }));
    });
  }

  /** QA (F11.15): um Claude numa aba, no worktree da tarefa, pergunta onde juntar e confere. */
  function qa(card: KanbanCard) {
    void act(async () => {
      requireClaudeLogin("QA");
      const info = await client.qaCard(profile, project, card.id);
      useAppStore.setState((s) => ({
        sessions: [...s.sessions.filter((x) => x.id !== info.id), info],
        toast: `QA de ${card.id} aberto: responda na aba onde juntar (main ou outra branch).`,
      }));
    });
  }

  /** "Retomar" (F11.14): a cota voltou; o orquestrador volta a puxar e avisa o que abriu. */
  function resume() {
    void act(async () => {
      const after = await client.resumeOrchestrator(profile, project);
      setBoard(after);
      const started = after.started ?? [];
      useAppStore.setState({
        toast: started.length
          ? `Orquestrador retomado: ${started.join(", ")} foi para trabalhador.`
          : "Orquestrador retomado.",
      });
    });
  }

  /** Encerra o chat do backlog (a sessão some do projeto). Sessão já morta não é erro. */
  async function closeChat(sessionId: string) {
    useAppStore.setState({ boardChat: null });
    await client.deleteSession(sessionId).catch(() => undefined);
    await refresh();
  }

  async function saveCard(draft: KanbanDraft) {
    if (editing === "novo") {
      await client.createCard(profile, project, draft);
    } else if (editing) {
      await client.updateCard(profile, project, editing.id, draft);
    }
    setEditing(null);
    await refresh();
  }

  const cards = board?.cards ?? [];
  const orchestrator: KanbanOrchestrator = board?.orchestrator ?? {
    enabled: false,
    max_workers: 1,
  };

  /** Muda o orquestrador automático (F11.11) e avisa o que aconteceu; ligado, o hubd já abre
   * os primeiros trabalhadores antes de responder. */
  async function saveOrchestrator(change: Partial<KanbanOrchestrator>) {
    const next = { ...orchestrator, ...change };
    const after = await client.setOrchestrator(profile, project, next);
    setBoard(after);
    useAppStore.setState({ toast: orchestratorNotice(orchestrator, after) });
  }

  function setOrchestrator(enabled: boolean, maxWorkers: number) {
    if (enabled !== orchestrator.enabled) setSwitching(enabled ? "Ligando…" : "Desligando…");
    void act(() => saveOrchestrator({ enabled, max_workers: maxWorkers })).finally(() =>
      setSwitching(null),
    );
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col" aria-label={`Quadro de ${project}`}>
      <header className="flex items-center gap-3 border-b border-line px-5 py-3">
        <LayoutGrid className="size-4 text-slate-300" />
        <h2 className="text-lg font-semibold text-white">Quadro · {project}</h2>
        {busy && (
          <span role="status" className="text-xs text-slate-400">
            Processando…
          </span>
        )}
        <div className="ml-auto flex gap-2">
          <div className="flex items-stretch overflow-hidden rounded-lg border border-line">
            <button
              type="button"
              role="switch"
              aria-checked={orchestrator.enabled}
              disabled={board === null}
              onClick={() => setOrchestrator(!orchestrator.enabled, orchestrator.max_workers)}
              title="Ligado, o hubd abre trabalhadores sozinho para os cards de agente do backlog"
              className={`flex items-center gap-1.5 px-3 py-1.5 text-sm ${
                orchestrator.enabled
                  ? "bg-violet-500/25 text-violet-100 hover:bg-violet-500/35"
                  : "bg-white/5 text-slate-200 hover:bg-white/10"
              }`}
            >
              <Bot className="size-4" />{" "}
              {switching ?? `Orquestrador ${orchestrator.enabled ? "ligado" : "desligado"}`}
            </button>
            {/* Mesmo padrão do select do CardDialog: sem aparência nativa, seta do tema. */}
            <span className="relative h-full border-l border-line">
              <select
                aria-label="Trabalhadores ao mesmo tempo"
                title="Quantas tarefas em fazendo ao mesmo tempo, no máximo"
                value={orchestrator.max_workers}
                disabled={board === null}
                onChange={(event) =>
                  setOrchestrator(orchestrator.enabled, Number(event.target.value))
                }
                className="h-full scheme-dark appearance-none bg-white/5 py-1.5 pr-7 pl-3 text-sm text-slate-200 outline-none hover:bg-white/10 focus:bg-white/10 [&>option]:bg-panel"
              >
                {Array.from({ length: MAX_WORKERS }, (_, i) => i + 1).map((n) => (
                  <option key={n} value={n}>
                    {n}×
                  </option>
                ))}
              </select>
              <ChevronDown className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 text-slate-400" />
            </span>
            <button
              type="button"
              disabled={board === null}
              onClick={() => setInstructionsOpen(true)}
              title={
                orchestrator.instructions
                  ? "Instruções extras dos trabalhadores (definidas)"
                  : "Instruções extras dos trabalhadores, somadas às regras padrão"
              }
              aria-label="Instruções dos trabalhadores"
              className={`border-l border-line px-2.5 hover:bg-white/10 ${
                orchestrator.instructions
                  ? "bg-violet-500/25 text-violet-100"
                  : "bg-white/5 text-slate-300"
              }`}
            >
              <ScrollText className="size-4" />
            </button>
          </div>
          <button
            type="button"
            onClick={() =>
              void act(async () => {
                requireClaudeLogin("Popular pelo backlog");
                // Um chat por projeto: o anterior encerra antes, para não sobrar sessão sem tela.
                if (chat) await client.deleteSession(chat.sessionId).catch(() => undefined);
                const info = await client.explorarBacklog(profile, project);
                // O chat mora no quadro (sem aba): a sessão entra na lista só para os nomes
                // aparecerem nos cards, e `visibleProjectSessions` a esconde das abas.
                useAppStore.setState((s) => ({
                  sessions: [...s.sessions.filter((x) => x.id !== info.id), info],
                  boardChat: { key: viewKey(profile, project), sessionId: info.id },
                  toast:
                    "Claude lendo o backlog: o chat aparece aqui, e os cards conforme ele cria.",
                }));
              })
            }
            title="Abre um Claude que lê o backlog (os .md do projeto) e cria os cards"
            className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-200 hover:bg-white/10"
          >
            <Sparkles className="size-4" /> Popular pelo backlog
          </button>
          <button
            type="button"
            onClick={() => setEditing("novo")}
            className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-200 hover:bg-white/10"
          >
            <Plus className="size-4" /> Novo card
          </button>
        </div>
      </header>
      {error && (
        <p className="border-b border-line bg-rose-950/40 px-5 py-2 font-mono text-xs text-rose-300">
          {error}
        </p>
      )}
      {orchestrator.paused && (
        <div
          role="alert"
          className="flex items-center gap-3 border-b border-line bg-amber-950/50 px-5 py-2 text-sm text-amber-100"
        >
          <CirclePause className="size-4 shrink-0" />
          <p className="min-w-0 flex-1">
            Cota do provedor esgotada às {clock(orchestrator.paused.at)}: orquestrador pausado, nada
            novo abre sozinho. Termine os cards parados com outro harness, ou retome quando a cota
            voltar.
            <span className="block truncate font-mono text-xs text-amber-200/80">
              {orchestrator.paused.reason}
            </span>
          </p>
          <button
            type="button"
            onClick={resume}
            title="A cota voltou: o orquestrador volta a puxar, inclusive os cards parados por cota"
            className="flex shrink-0 items-center gap-1.5 rounded-lg border border-amber-300/40 px-3 py-1 text-xs hover:bg-amber-300/10"
          >
            <Play className="size-3.5" /> Retomar
          </button>
        </div>
      )}
      {orchestrator.enabled && board?.orchestrator_error && (
        <p className="border-b border-line bg-amber-950/40 px-5 py-2 font-mono text-xs text-amber-200">
          Orquestrador: {board.orchestrator_error}
        </p>
      )}
      <div className="flex min-h-0 flex-1 flex-col">
        {board === null ? (
          <p className="p-6 text-sm text-slate-400">Lendo o quadro…</p>
        ) : (
          <div className="grid min-h-0 flex-1 grid-cols-4 grid-rows-[minmax(0,1fr)] gap-3 p-4">
            {columns.map((column) => {
              const inColumn = cards.filter((card) => card.column === column.id);
              const target = overColumn === column.id;
              return (
                <section
                  key={column.id}
                  aria-label={`Coluna ${column.label}`}
                  onDragOver={(event: DragEvent) => {
                    if (!dragging) return;
                    event.preventDefault();
                    setOverColumn(column.id);
                  }}
                  onDragLeave={() => setOverColumn((c) => (c === column.id ? null : c))}
                  onDrop={(event: DragEvent) => {
                    event.preventDefault();
                    setOverColumn(null);
                    if (dragging) moveTo(dragging, column.id);
                    setDragging(null);
                  }}
                  className={`flex min-h-0 flex-col gap-2 overflow-y-auto rounded-xl border p-3 transition-colors ${
                    target ? "border-accent/60 bg-accent/10" : "border-line bg-black/20"
                  }`}
                >
                  <h3 className="flex items-center gap-2 text-sm font-semibold text-slate-200">
                    {column.label}
                    <span className="rounded-full bg-white/10 px-2 text-xs text-slate-400">
                      {inColumn.length}
                    </span>
                  </h3>
                  {inColumn.map((card) => (
                    <article
                      key={card.id}
                      aria-label={`${card.id} ${card.title}`}
                      draggable
                      onDragStart={(event: DragEvent) => {
                        event.dataTransfer.setData("text/plain", card.id);
                        setDragging(card);
                      }}
                      onDragEnd={() => {
                        setDragging(null);
                        setOverColumn(null);
                      }}
                      className="flex cursor-grab flex-col gap-2 rounded-lg border border-line bg-surface-strong p-3 text-sm active:cursor-grabbing"
                    >
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-xs text-slate-400">{card.id}</span>
                        <span
                          className={`rounded border px-1.5 text-[10px] uppercase ${priorityStyle[card.priority]}`}
                        >
                          {card.priority}
                        </span>
                        <span
                          className={`rounded px-1.5 text-[10px] uppercase ${
                            card.kind === "agente"
                              ? "bg-violet-500/20 text-violet-200"
                              : "bg-white/10 text-slate-300"
                          }`}
                        >
                          {card.kind === "agente" ? "agente" : "manual"}
                        </span>
                        {card.blocked && (
                          <span className="rounded bg-amber-500/20 px-1.5 text-[10px] text-amber-200 uppercase">
                            cota
                          </span>
                        )}
                      </div>
                      <button
                        type="button"
                        onClick={() => setEditing(card)}
                        className="text-left font-medium text-white hover:underline"
                      >
                        {card.title}
                      </button>
                      {card.description && (
                        <p className="line-clamp-3 whitespace-pre-line text-xs text-slate-400">
                          {card.description}
                        </p>
                      )}
                      {card.holder && (
                        <p className="text-xs text-slate-300">
                          em {holderLabel(card.holder, sessions)}
                        </p>
                      )}
                      {column.id === "backlog" && (card.failures ?? 0) > 0 && (
                        <p className="text-xs text-amber-300">
                          Voltou {card.failures}× sem concluir: o orquestrador não puxa de novo
                          sozinho.
                        </p>
                      )}
                      {column.id === "backlog" && card.blocked && (
                        <p className="text-xs text-amber-300">
                          Parou por cota: <span className="font-mono">{card.blocked}</span>
                        </p>
                      )}
                      {card.merged ? (
                        <p className="text-xs text-emerald-300">
                          na <span className="font-mono">{card.merged.branch}</span> ·{" "}
                          <span className="font-mono">{card.merged.commit.slice(0, 7)}</span>
                        </p>
                      ) : (
                        card.commit && (
                          <p className="font-mono text-xs text-emerald-300">
                            {card.commit.slice(0, 7)}
                          </p>
                        )
                      )}
                      <div className="flex flex-wrap gap-1.5 pt-1">
                        {column.id === "backlog" && card.kind === "manual" && (
                          <button
                            type="button"
                            onClick={() =>
                              void act(() => client.mandarCard(profile, project, card.id))
                            }
                            title="Manda esta tarefa para um trabalhador agora"
                            className="flex items-center gap-1 rounded-md border border-accent/50 bg-accent/20 px-2 py-1 text-xs text-white hover:bg-accent/30"
                          >
                            <Play className="size-3.5" /> Mandar
                          </button>
                        )}
                        {column.id === "backlog" &&
                          card.kind === "agente" &&
                          (card.failures ?? 0) > 0 && (
                            <button
                              type="button"
                              onClick={() =>
                                void act(() =>
                                  client.updateCard(profile, project, card.id, { failures: 0 }),
                                )
                              }
                              title="Zera as voltas: o orquestrador automático pode puxar de novo"
                              className="flex items-center gap-1 rounded-md border border-line px-2 py-1 text-xs text-slate-200 hover:bg-white/10"
                            >
                              <RotateCcw className="size-3.5" /> Tentar de novo
                            </button>
                          )}
                        {/* Parou por cota ou voltou sem concluir: dá para terminar com outro harness. */}
                        {column.id === "backlog" && (card.blocked || (card.failures ?? 0) > 0) && (
                          <>
                            <button
                              type="button"
                              onClick={() => finishWithClaude(card)}
                              title="Abre um Claude Code no worktree da tarefa, que continua de onde o trabalhador parou"
                              className="flex items-center gap-1 rounded-md border border-accent/50 bg-accent/20 px-2 py-1 text-xs text-white hover:bg-accent/30"
                            >
                              <Sparkles className="size-3.5" /> Terminar com Claude
                            </button>
                            <button
                              type="button"
                              onClick={() =>
                                setOtherModel(
                                  otherModel?.id === card.id
                                    ? null
                                    : { id: card.id, model: OTHER_MODEL },
                                )
                              }
                              title="Manda para um trabalhador do opencode com outro modelo (ex.: pelo 9router)"
                              className="flex items-center gap-1 rounded-md border border-line px-2 py-1 text-xs text-slate-200 hover:bg-white/10"
                            >
                              <Play className="size-3.5" /> Outro modelo…
                            </button>
                          </>
                        )}
                        {column.id === "feito" && !card.holder && (
                          <button
                            type="button"
                            onClick={() => qa(card)}
                            title="Abre um Claude que pergunta onde juntar, confere se atende o pedido e valida ou devolve ao backlog com o que falta"
                            className="flex items-center gap-1 rounded-md border border-accent/50 bg-accent/20 px-2 py-1 text-xs text-white hover:bg-accent/30"
                          >
                            <ShieldCheck className="size-3.5" /> QA
                          </button>
                        )}
                        {column.id === "fazendo" && card.holder && (
                          <button
                            type="button"
                            onClick={() => {
                              const holder = card.holder ?? "";
                              void act(() => client.restartWorker(holder));
                            }}
                            title="Relança o trabalhador com o mesmo modelo, no mesmo worktree"
                            className="flex items-center gap-1 rounded-md border border-line px-2 py-1 text-xs text-slate-200 hover:bg-white/10"
                          >
                            <RotateCcw className="size-3.5" /> Reiniciar
                          </button>
                        )}
                        {column.id === "fazendo" && card.holder && (
                          <button
                            type="button"
                            onClick={() => {
                              const holder = card.holder ?? "";
                              void act(() => client.deleteSession(holder));
                            }}
                            title="Para o trabalhador e devolve a tarefa ao backlog; o worktree e os commits ficam"
                            className="flex items-center gap-1 rounded-md border border-line px-2 py-1 text-xs text-slate-200 hover:bg-white/10"
                          >
                            <Square className="size-3.5" /> Parar
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={() => setEditing(card)}
                          title="Editar"
                          className="rounded-md p-1 text-slate-400 hover:text-white"
                        >
                          <Pencil className="size-3.5" />
                        </button>
                        <button
                          type="button"
                          title="Apagar o card"
                          onClick={() => {
                            if (window.confirm(`Apagar ${card.id} "${card.title}"?`)) {
                              void act(() => client.deleteCard(profile, project, card.id));
                            }
                          }}
                          className="rounded-md p-1 text-slate-400 hover:text-rose-300"
                        >
                          <Trash2 className="size-3.5" />
                        </button>
                      </div>
                      {otherModel?.id === card.id && (
                        <form
                          className="flex gap-1.5"
                          onSubmit={(event) => {
                            event.preventDefault();
                            const model = otherModel.model.trim();
                            if (!model) return;
                            setOtherModel(null);
                            void act(() => client.mandarCard(profile, project, card.id, [model]));
                          }}
                        >
                          <input
                            aria-label={`Modelo do opencode para ${card.id}`}
                            value={otherModel.model}
                            onChange={(event) =>
                              setOtherModel({ id: card.id, model: event.target.value })
                            }
                            className="min-w-0 flex-1 rounded-md border border-line bg-black/30 px-2 py-1 font-mono text-xs text-slate-100 outline-none focus:border-accent/60"
                          />
                          <button
                            type="submit"
                            className="rounded-md border border-accent/50 bg-accent/20 px-2 py-1 text-xs text-white hover:bg-accent/30"
                          >
                            Mandar
                          </button>
                        </form>
                      )}
                    </article>
                  ))}
                </section>
              );
            })}
          </div>
        )}
        {chat && (
          <div className="flex h-[45vh] min-h-[16rem] shrink-0 flex-col border-t border-line">
            <div className="flex items-center gap-2 px-4 py-1.5 text-xs text-slate-300">
              <Sparkles className="size-3.5" /> Chat do backlog
              <button
                type="button"
                title="Encerrar o chat (o Claude para de ler o backlog)"
                onClick={() => void closeChat(chat.sessionId)}
                className="ml-auto rounded p-0.5 text-slate-400 hover:text-white"
              >
                <X className="size-3.5" />
              </button>
            </div>
            <TerminalView sessionId={chat.sessionId} />
          </div>
        )}
      </div>
      {instructionsOpen && (
        <OrchestratorDialog
          instructions={orchestrator.instructions ?? ""}
          onSave={async (instructions) => {
            await saveOrchestrator({ instructions });
            setInstructionsOpen(false);
          }}
          onClose={() => setInstructionsOpen(false)}
        />
      )}
      {editing && (
        <CardDialog
          target={editing === "novo" ? null : editing}
          onSave={saveCard}
          onClose={() => setEditing(null)}
        />
      )}
    </section>
  );
}
