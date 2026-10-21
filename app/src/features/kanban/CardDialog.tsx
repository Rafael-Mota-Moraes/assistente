import { ChevronDown, X } from "lucide-react";
import { type FormEvent, useState } from "react";
import type { KanbanCard, KanbanDraft, KanbanKind, KanbanPriority } from "../../bridge/types";

const kinds: { id: KanbanKind; label: string; detail: string }[] = [
  {
    id: "manual",
    label: "Manual",
    detail: "Só roda quando você manda (botão Mandar).",
  },
  {
    id: "agente",
    label: "Para o agente",
    detail: "Marcado para o agente implementar: o orquestrador puxa sozinho.",
  },
];

const priorities: { id: KanbanPriority; label: string }[] = [
  { id: "alta", label: "Alta" },
  { id: "normal", label: "Normal" },
  { id: "baixa", label: "Baixa" },
];

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Criar ou editar um card do quadro (F11.3). `target` = `null` cria um card novo. */
export function CardDialog({
  target,
  onSave,
  onClose,
}: {
  target: KanbanCard | null;
  onSave: (draft: KanbanDraft) => Promise<void>;
  onClose: () => void;
}) {
  const [title, setTitle] = useState(target?.title ?? "");
  const [description, setDescription] = useState(target?.description ?? "");
  const [priority, setPriority] = useState<KanbanPriority>(target?.priority ?? "normal");
  const [kind, setKind] = useState<KanbanKind>(target?.kind ?? "manual");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!title.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await onSave({ title, description, priority, kind });
    } catch (e) {
      setError(message(e));
      setSaving(false);
    }
  }

  return (
    <div
      role="dialog"
      aria-label={target ? `Editar ${target.id}` : "Novo card"}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onKeyDown={(event) => {
        if (event.key === "Escape") onClose();
      }}
    >
      <form
        onSubmit={submit}
        className="flex max-h-full w-full max-w-lg flex-col gap-4 overflow-y-auto rounded-2xl border border-line bg-surface-strong p-6 shadow-2xl"
      >
        <header className="flex items-center gap-3">
          <h2 className="text-lg font-semibold text-white">
            {target ? `Editar ${target.id}` : "Novo card"}
          </h2>
          <button
            type="button"
            onClick={onClose}
            title="Fechar"
            className="ml-auto rounded p-1 text-slate-400 hover:text-white"
          >
            <X className="size-4" />
          </button>
        </header>

        <label className="flex flex-col gap-1 text-sm text-slate-300">
          Título
          <input
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            maxLength={200}
            className="rounded-lg border border-line bg-black/30 px-3 py-2 text-white outline-none focus:border-accent/60"
          />
        </label>

        <label className="flex flex-col gap-1 text-sm text-slate-300">
          Descrição
          <textarea
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            rows={6}
            placeholder="O que fazer, onde, o que conta como pronto. É o pedido que o agente recebe."
            className="rounded-lg border border-line bg-black/30 px-3 py-2 font-mono text-sm text-white outline-none focus:border-accent/60"
          />
        </label>

        <label className="flex flex-col gap-1 text-sm text-slate-300">
          Prioridade
          <span className="relative">
            {/* Sem aparência nativa: a lista e a seta seguem o tema escuro do app. */}
            <select
              value={priority}
              onChange={(event) => setPriority(event.target.value as KanbanPriority)}
              className="w-full scheme-dark appearance-none rounded-lg border border-line bg-black/30 py-2 pr-9 pl-3 text-white outline-none focus:border-accent/60 [&>option]:bg-panel"
            >
              {priorities.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.label}
                </option>
              ))}
            </select>
            <ChevronDown className="pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2 text-slate-400" />
          </span>
        </label>

        <fieldset className="flex flex-col gap-2 text-sm text-slate-300">
          <legend className="mb-1">Tipo</legend>
          {kinds.map((k) => (
            <label
              key={k.id}
              className={`flex cursor-pointer gap-3 rounded-lg border px-3 py-2 ${
                kind === k.id ? "border-accent/60 bg-accent/10" : "border-line bg-black/20"
              }`}
            >
              <input
                type="radio"
                name="tipo"
                value={k.id}
                checked={kind === k.id}
                onChange={() => setKind(k.id)}
                className="mt-1"
              />
              <span className="flex flex-col">
                <span className="font-medium text-white">{k.label}</span>
                <span className="text-xs text-slate-400">{k.detail}</span>
              </span>
            </label>
          ))}
        </fieldset>

        {error && <p className="font-mono text-xs text-rose-300">{error}</p>}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-line px-4 py-2 text-sm text-slate-300 hover:bg-white/10"
          >
            Cancelar
          </button>
          <button
            type="submit"
            disabled={saving || !title.trim()}
            className="rounded-lg bg-accent/80 px-4 py-2 text-sm font-medium text-white hover:bg-accent disabled:opacity-40"
          >
            {saving ? "Salvando…" : "Salvar"}
          </button>
        </div>
      </form>
    </div>
  );
}
