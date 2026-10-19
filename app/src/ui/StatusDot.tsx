export type Tone = "ok" | "warn" | "off" | "idle";

const toneClass: Record<Tone, string> = {
  ok: "bg-emerald-400 shadow-[0_0_8px] shadow-emerald-400/70",
  warn: "bg-amber-400 shadow-[0_0_8px] shadow-amber-400/60",
  off: "bg-rose-500",
  idle: "bg-slate-500",
};

export function StatusDot({ tone, label, title }: { tone: Tone; label: string; title?: string }) {
  return (
    <span className="flex items-center gap-1.5" title={title} data-tone={tone}>
      <span className={`size-2 rounded-full ${toneClass[tone]}`} />
      <span>{label}</span>
    </span>
  );
}
