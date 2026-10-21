import { X } from "lucide-react";
import { type FormEvent, useState } from "react";

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Instruções dos trabalhadores do projeto (F11.11): vão depois das regras padrão do prompt. */
export function OrchestratorDialog({
  instructions,
  onSave,
  onClose,
}: {
  instructions: string;
  onSave: (instructions: string) => Promise<void>;
  onClose: () => void;
}) {
  const [text, setText] = useState(instructions);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    setError(null);
    try {
      await onSave(text);
    } catch (e) {
      setError(message(e));
      setSaving(false);
    }
  }

  return (
    <div
      role="dialog"
      aria-label="Instruções dos trabalhadores"
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
          <h2 className="text-lg font-semibold text-white">Instruções dos trabalhadores</h2>
          <button
            type="button"
            onClick={onClose}
            title="Fechar"
            className="ml-auto rounded p-1 text-slate-400 hover:text-white"
          >
            <X className="size-4" />
          </button>
        </header>

        <p className="text-sm text-slate-400">
          Entram no fim do prompt de cada trabalhador deste projeto (orquestrador, Mandar e
          Reiniciar), depois das regras padrão: worktree próprio, commits pequenos, rodar as
          verificações e deixar a árvore limpa. Valem para os próximos que abrirem.
        </p>

        <label className="flex flex-col gap-1 text-sm text-slate-300">
          Instruções extras
          <textarea
            value={text}
            onChange={(event) => setText(event.target.value)}
            rows={8}
            maxLength={10000}
            placeholder="Ex.: use pnpm, nunca npm. Escreva os testes antes. Não mexa em migrations."
            className="rounded-lg border border-line bg-black/30 px-3 py-2 font-mono text-sm text-white outline-none focus:border-accent/60"
          />
        </label>

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
            disabled={saving}
            className="rounded-lg bg-accent/80 px-4 py-2 text-sm font-medium text-white hover:bg-accent disabled:opacity-40"
          >
            {saving ? "Salvando…" : "Salvar"}
          </button>
        </div>
      </form>
    </div>
  );
}
