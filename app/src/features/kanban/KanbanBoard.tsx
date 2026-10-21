import { LayoutGrid, Pencil, Play, Plus, RotateCcw, Trash2, X } from "lucide-react";
import { type DragEvent, useCallback, useEffect, useState } from "react";
import type {
  KanbanBoard as Board,
  KanbanCard,
  KanbanColumn,
  KanbanDraft,
  KanbanPriority,
  SessionInfo,
} from "../../bridge/types";
import { useAppStore } from "../../store";
import { tabLabel } from "../sessions/SessionArea";
import { CardDialog } from "./CardDialog";

/** Quadro é lido de novo a cada 2 s (F11.3): o que os agentes puxam aparece sem recarregar. */
const EVERY_MS = 2000;

const columns: { id: KanbanColumn; label: string }[] = [
  { id: "backlog", label: "Backlog" },
  { id: "fazendo", label: "Fazendo" },
  { id: "feito", label: "Feito" },
];

const priorityStyle: Record<KanbanPriority, string> = {
  alta: "border-rose-400/50 text-rose-200",
  normal: "border-line text-slate-300",
  baixa: "border-sky-400/40 text-sky-200",
};

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Quem segura a tarefa: a aba da sessão, ou o id cru se a aba já não está aberta. */
function holderLabel(holder: string, sessions: SessionInfo[]): string {
  const session = sessions.find((s) => s.id === holder);
  return session ? tabLabel(session) : holder;
}

/** Quadro do projeto ativo (F11.3): backlog · fazendo · feito, no lugar da área de sessões. */
export function KanbanBoard({ onClose }: { onClose: () => void }) {
  const hub = useAppStore((s) => s.hub);
  const activeProfile = useAppStore((s) => s.activeProfile);
  const activeProject = useAppStore((s) => s.activeProject);
  const sessions = useAppStore((s) => s.sessions);
  const [board, setBoard] = useState<Board | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<KanbanCard | "novo" | null>(null);
  const [dragging, setDragging] = useState<KanbanCard | null>(null);
  const [overColumn, setOverColumn] = useState<KanbanColumn | null>(null);

  const refresh = useCallback(async () => {
    if (!hub || !activeProfile || !activeProject) return;
    try {
      setBoard(await hub.kanban(activeProfile, activeProject));
      setError(null);
    } catch (e) {
      setError(message(e));
    }
  }, [hub, activeProfile, activeProject]);

  useEffect(() => {
    setBoard(null);
    void refresh();
    const timer = setInterval(() => void refresh(), EVERY_MS);
    return () => clearInterval(timer);
  }, [refresh]);

  if (!hub || !activeProfile || !activeProject) return null;
  const client = hub;
  const profile = activeProfile;
  const project = activeProject;

  /** Roda uma ação na API, mostra o erro se falhar e atualiza o quadro. */
  async function act(action: () => Promise<unknown>) {
    try {
      await action();
      setError(null);
    } catch (e) {
      setError(message(e));
    }
    await refresh();
  }

  function moveTo(card: KanbanCard, column: KanbanColumn) {
    if (card.column === column) return;
    void act(() => client.updateCard(profile, project, card.id, { column }));
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

  return (
    <section className="flex min-h-0 flex-1 flex-col" aria-label={`Quadro de ${project}`}>
      <header className="flex items-center gap-3 border-b border-line px-5 py-3">
        <LayoutGrid className="size-4 text-slate-300" />
        <h2 className="text-lg font-semibold text-white">Quadro · {project}</h2>
        <div className="ml-auto flex gap-2">
          <button
            type="button"
            onClick={() => setEditing("novo")}
            className="flex items-center gap-1.5 rounded-lg border border-line bg-white/5 px-3 py-1.5 text-sm text-slate-200 hover:bg-white/10"
          >
            <Plus className="size-4" /> Novo card
          </button>
          <button
            type="button"
            onClick={onClose}
            title="Voltar às sessões"
            className="rounded-lg border border-line bg-white/5 p-1.5 text-slate-300 hover:bg-white/10"
          >
            <X className="size-4" />
          </button>
        </div>
      </header>
      {error && (
        <p className="border-b border-line bg-rose-950/40 px-5 py-2 font-mono text-xs text-rose-300">
          {error}
        </p>
      )}
      {board === null ? (
        <p className="p-6 text-sm text-slate-400">Lendo o quadro…</p>
      ) : (
        <div className="grid min-h-0 flex-1 grid-cols-3 gap-3 overflow-y-auto p-4">
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
                className={`flex min-h-0 flex-col gap-2 rounded-xl border p-3 transition-colors ${
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
                    {card.commit && (
                      <p className="font-mono text-xs text-emerald-300">
                        {card.commit.slice(0, 7)}
                      </p>
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
                  </article>
                ))}
              </section>
            );
          })}
        </div>
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
