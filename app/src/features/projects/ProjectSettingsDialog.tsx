import { Plus, Settings, TriangleAlert, X } from "lucide-react";
import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import type { ProjectSettings } from "../../bridge/types";
import { useAppStore } from "../../store";

function close() {
  useAppStore.setState({ projectSettings: null });
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Engrenagem do projeto (F9.3): afrouxa o ai-jail das próximas sessões deste projeto. */
export function ProjectSettingsDialog() {
  const target = useAppStore((s) => s.projectSettings);
  const hub = useAppStore((s) => s.hub);
  const [settings, setSettings] = useState<ProjectSettings | null>(null);
  const [dockerFromFile, setDockerFromFile] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setSettings(null);
    setError(null);
    if (!hub || !target) return;
    let cancelled = false;
    hub
      .projectSettings(target.profile, target.project)
      .then((view) => {
        if (cancelled) return;
        setSettings(view.settings);
        setDockerFromFile(view.docker_from_project_file);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(message(e));
      });
    return () => {
      cancelled = true;
    };
  }, [hub, target]);

  if (!target) return null;

  const patch = (change: Partial<ProjectSettings>) =>
    setSettings((current) => (current ? { ...current, ...change } : current));

  async function save() {
    if (!hub || !target || !settings) return;
    setSaving(true);
    try {
      await hub.saveProjectSettings(target.profile, target.project, {
        ...settings,
        // Campo vazio = sem preview (o hubd guarda `null`).
        preview_url: settings.preview_url?.trim() || null,
        // Campo vazio = o padrão (o hubd recusa check vazio).
        check: settings.check.trim() || "./dev just check",
      });
      useAppStore.setState({
        projectSettings: null,
        toast: `Configurações de ${target.project} salvas: valem para as próximas sessões.`,
      });
    } catch (e) {
      setError(message(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div
      role="dialog"
      aria-label={`Configurações de ${target.project}`}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      onClick={(event) => {
        if (event.target === event.currentTarget) close();
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") close();
      }}
    >
      <div className="flex max-h-full w-full max-w-xl flex-col gap-4 overflow-y-auto rounded-2xl border border-line bg-surface-strong p-6 shadow-2xl">
        <header className="flex items-center gap-3">
          <Settings className="size-5 shrink-0 text-slate-300" />
          <h2 className="text-lg font-semibold text-white">
            {target.project}
            <span className="ml-2 text-sm font-normal text-slate-400">sessões e preview</span>
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

        <p className="text-sm text-slate-400">
          O que o agente pode fazer dentro do ai-jail neste projeto e onde ver o projeto rodando.
          Fica na VM, fora do repo (o agente não muda isso sozinho): o jail vale para as{" "}
          <strong>sessões novas</strong>, o preview vale na hora.
        </p>

        {error && (
          <p className="rounded-lg bg-rose-950/50 px-3 py-2 font-mono text-xs text-rose-300">
            {error}
          </p>
        )}

        {!settings && !error && <p className="text-sm text-slate-400">Carregando…</p>}

        {settings && (
          <>
            <Toggle
              label="Docker"
              checked={settings.docker || dockerFromFile}
              disabled={dockerFromFile}
              onChange={(docker) => patch({ docker })}
            >
              Para <code className="font-mono">./dev</code> e{" "}
              <code className="font-mono">docker</code>. Equivale a root na VM.
              {dockerFromFile && (
                <span className="block text-amber-300">
                  Ligado pelo <code className="font-mono">[jail]</code> do .assistente.toml do
                  projeto.
                </span>
              )}
            </Toggle>

            <Toggle
              label="Rede completa"
              checked={settings.full_network}
              onChange={(full_network) => patch({ full_network })}
            >
              Desliga o filtro de hosts.
              {settings.full_network && (
                <span className="mt-1 flex items-start gap-1.5 rounded-lg bg-rose-950/50 px-2 py-1.5 text-rose-300">
                  <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />O agente alcança qualquer
                  site, a sua rede local e os serviços da VM, e pode enviar para fora tudo o que lê.
                  Prefira liberar só os hosts abaixo.
                </span>
              )}
            </Toggle>

            <ListEditor
              label="Hosts liberados"
              hint="Só o nome (pypi.org); vale também para os subdomínios."
              placeholder="pypi.org"
              items={settings.allow_hosts}
              disabled={settings.full_network}
              onChange={(allow_hosts) => patch({ allow_hosts })}
            />

            <ListEditor
              label="Comandos sempre permitidos"
              hint="Pedidos do agente para rodar fora do jail que não precisam de aprovação (texto exato). Cuidado: se o comando roda arquivos do projeto (./dev, make, scripts), o agente pode editá-los e escapar sem perguntar."
              placeholder="cargo build"
              mono
              items={settings.allowed_commands}
              onChange={(allowed_commands) => patch({ allowed_commands })}
            />

            <label className="flex flex-col gap-1 text-sm">
              <span className="font-medium text-slate-200">URL do preview</span>
              <input
                aria-label="URL do preview"
                value={settings.preview_url ?? ""}
                onChange={(event) => patch({ preview_url: event.target.value })}
                placeholder="http://10.77.0.10:5173"
                className="rounded-lg border border-line bg-black/30 px-2 py-1 font-mono text-slate-200 placeholder:text-slate-600"
              />
              <span className="text-xs text-slate-400">
                Endereço do dev server <strong>visto do PC</strong>: se o projeto roda na VM, use o
                IP dela (o hubd responde no mesmo IP) e escute o dev server em 0.0.0.0. Só{" "}
                <code className="font-mono">http</code>/<code className="font-mono">https</code> e
                sem senha. Fica na VM, fora do repo.
              </span>
            </label>

            <label className="flex flex-col gap-1 text-sm">
              <span className="font-medium text-slate-200">Check do QA</span>
              <input
                aria-label="Check do QA"
                value={settings.check}
                onChange={(event) => patch({ check: event.target.value })}
                placeholder="./dev just check"
                className="rounded-lg border border-line bg-black/30 px-2 py-1 font-mono text-slate-200 placeholder:text-slate-600"
              />
              <span className="text-xs text-slate-400">
                O <code className="font-mono">kanban validar</code> só junta a tarefa com este
                comando verde. Roda <strong>no PC</strong>, no commit que a branch vai receber,
                depois que você aprova o pedido. Fica aqui, fora do repo: o agente não troca.
              </span>
            </label>

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
                onClick={save}
                disabled={saving}
                className="rounded-lg bg-violet-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-violet-500 disabled:opacity-50"
              >
                Salvar
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function Toggle({
  label,
  checked,
  disabled,
  onChange,
  children,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (value: boolean) => void;
  children: ReactNode;
}) {
  return (
    <label className="flex items-start gap-3 text-sm">
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        className="mt-1 accent-violet-500"
      />
      <span>
        <span className="font-medium text-slate-200">{label}</span>
        <span className="block text-xs text-slate-400">{children}</span>
      </span>
    </label>
  );
}

function ListEditor({
  label,
  hint,
  placeholder,
  items,
  mono,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  placeholder: string;
  items: string[];
  mono?: boolean;
  disabled?: boolean;
  onChange: (items: string[]) => void;
}) {
  const [draft, setDraft] = useState("");
  const font = mono ? "font-mono" : "";

  function add(event: FormEvent) {
    event.preventDefault();
    const value = draft.trim();
    if (!value || items.includes(value)) return;
    onChange([...items, value]);
    setDraft("");
  }

  return (
    <section aria-label={label} className={`text-sm ${disabled ? "opacity-50" : ""}`}>
      <h3 className="font-medium text-slate-200">{label}</h3>
      <p className="text-xs text-slate-400">{hint}</p>
      <ul className="mt-2 flex flex-col gap-1">
        {items.map((item) => (
          <li
            key={item}
            className="flex items-center gap-2 rounded-lg bg-black/30 px-2 py-1 text-slate-200"
          >
            <span className={`flex-1 truncate ${font}`}>{item}</span>
            <button
              type="button"
              title={`Remover ${item}`}
              disabled={disabled}
              onClick={() => onChange(items.filter((x) => x !== item))}
              className="rounded p-0.5 text-slate-500 hover:text-rose-300"
            >
              <X className="size-3.5" />
            </button>
          </li>
        ))}
      </ul>
      <form onSubmit={add} className="mt-2 flex gap-2">
        <input
          aria-label={`Novo em ${label}`}
          value={draft}
          disabled={disabled}
          placeholder={placeholder}
          onChange={(event) => setDraft(event.target.value)}
          className={`flex-1 rounded-lg border border-line bg-black/30 px-2 py-1 text-slate-200 ${font}`}
        />
        <button
          type="submit"
          title={`Adicionar em ${label}`}
          disabled={disabled}
          className="rounded-lg border border-line bg-white/5 px-2 text-slate-200 hover:bg-white/10"
        >
          <Plus className="size-4" />
        </button>
      </form>
    </section>
  );
}
