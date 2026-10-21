import { MergeView } from "@codemirror/merge";
import { oneDark } from "@codemirror/theme-one-dark";
import { useEffect, useRef, useState } from "react";
import type { GitDiff } from "../../bridge/types";
import { useAppStore } from "../../store";

export function DiffView({
  profile,
  project,
  path,
  from,
  to,
}: {
  profile: string;
  project: string;
  path: string;
  from?: string;
  to?: string;
}) {
  const hub = useAppStore((s) => s.hub);
  const [data, setData] = useState<GitDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const mvRef = useRef<MergeView | null>(null);

  useEffect(() => {
    if (!hub) return;
    let cancelled = false;
    setError(null);
    setData(null);
    hub
      .gitDiff(profile, project, path, from, to)
      .then((d) => {
        if (!cancelled) setData(d);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [hub, profile, project, path, from, to]);

  useEffect(() => {
    if (!ref.current || !data) return;
    if (mvRef.current) {
      mvRef.current.destroy();
      mvRef.current = null;
    }
    const mv = new MergeView({
      a: { doc: data.head ?? "", extensions: [oneDark] },
      b: { doc: data.working ?? "", extensions: [oneDark] },
      parent: ref.current,
    });
    mvRef.current = mv;
    return () => {
      mv.destroy();
      mvRef.current = null;
    };
  }, [data]);

  if (error) return <div className="p-4 text-sm text-rose-300">{error}</div>;
  if (!data) return <div className="p-4 text-sm text-slate-400">Carregando diff…</div>;
  return <div ref={ref} className="min-h-0 flex-1 overflow-hidden bg-[#282c34]" />;
}
