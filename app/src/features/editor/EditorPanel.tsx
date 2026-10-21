import { LanguageDescription } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { Compartment, EditorState } from "@codemirror/state";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorView, keymap, type ViewUpdate } from "@codemirror/view";
import { basicSetup } from "codemirror";
import {
  ChevronDown,
  ChevronRight,
  FileCode2,
  Folder,
  FolderOpen,
  GitCompare,
  RefreshCw,
  Save,
  X,
} from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { GitFileStatus } from "../../bridge/types";
import { useAppStore } from "../../store";
import { DiffView } from "./DiffView";

type TreeNode = { name: string; path: string; children: Map<string, TreeNode>; file: boolean };

function buildTree(files: string[]): TreeNode {
  const root: TreeNode = { name: "", path: "", children: new Map(), file: false };
  for (const f of files) {
    const parts = f.split("/");
    let cur = root;
    for (let i = 0; i < parts.length; i++) {
      const name = parts[i];
      const path = parts.slice(0, i + 1).join("/");
      const isFile = i === parts.length - 1;
      if (!cur.children.has(name)) {
        cur.children.set(name, { name, path, children: new Map(), file: isFile });
      }
      const child = cur.children.get(name);
      if (!child) break;
      cur = child;
      if (i === parts.length - 1) cur.file = true;
    }
  }
  return root;
}

function FileTree({
  files,
  status,
  selected,
  expanded,
  onToggle,
  onSelect,
}: {
  files: string[];
  status: { path: string; status: string }[];
  selected: string | null;
  expanded: Set<string>;
  onToggle: (folder: string) => void;
  onSelect: (path: string) => void;
}) {
  const root = buildTree(files);
  const render = (node: TreeNode, depth: number): ReactNode => {
    const children = Array.from(node.children.values()).sort((a, b) => {
      if (a.file !== b.file) return a.file ? 1 : -1;
      return a.name.localeCompare(b.name);
    });
    return children.map((child) => {
      if (child.file) {
        const st = status.find((s) => s.path === child.path);
        return (
          <li key={child.path}>
            <button
              type="button"
              onClick={() => onSelect(child.path)}
              style={{ paddingLeft: `${depth * 12 + 4}px` }}
              className={`flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-xs font-mono ${
                selected === child.path
                  ? "bg-white/10 text-white"
                  : "text-slate-300 hover:bg-white/5"
              }`}
            >
              <FileCode2 className="size-3.5 shrink-0 text-slate-500" />
              <span className="truncate">{child.name}</span>
              {st && (
                <span className="ml-auto shrink-0 text-[10px] text-amber-300">{st.status}</span>
              )}
            </button>
          </li>
        );
      }
      const isOpen = expanded.has(child.path);
      return (
        <li key={child.path}>
          <button
            type="button"
            onClick={() => onToggle(child.path)}
            style={{ paddingLeft: `${depth * 12 + 4}px` }}
            className="flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-xs font-mono text-slate-400 hover:bg-white/5"
          >
            {isOpen ? (
              <ChevronDown className="size-3.5 shrink-0" />
            ) : (
              <ChevronRight className="size-3.5 shrink-0" />
            )}
            {isOpen ? (
              <FolderOpen className="size-3.5 shrink-0 text-amber-300/70" />
            ) : (
              <Folder className="size-3.5 shrink-0 text-amber-300/70" />
            )}
            <span className="truncate">{child.name}</span>
          </button>
          {isOpen && <ul className="flex flex-col">{render(child, depth + 1)}</ul>}
        </li>
      );
    });
  };
  return <ul className="flex flex-col">{render(root, 0)}</ul>;
}

export function EditorPanel({ onClose }: { onClose?: () => void }) {
  const profile = useAppStore((s) => s.activeProfile);
  const project = useAppStore((s) => s.activeProject);
  const hub = useAppStore((s) => s.hub);
  const toast = (msg: string) => useAppStore.setState({ toast: msg });

  const [files, setFiles] = useState<string[]>([]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [leftWidth, setLeftWidth] = useState(256);
  const draggingRef = useRef(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [loading, setLoading] = useState(false);
  const [status, setStatus] = useState<GitFileStatus[]>([]);
  const [mode, setMode] = useState<"editor" | "diff">("editor");
  const [diffTarget, setDiffTarget] = useState<string | null>(null);

  const editorRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const loadingRef = useRef(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const langCompartment = useRef(new Compartment());

  useEffect(() => {
    function onMove(e: MouseEvent) {
      if (!draggingRef.current || !containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const newW = Math.min(520, Math.max(160, e.clientX - rect.left));
      setLeftWidth(newW);
    }
    function onUp() {
      draggingRef.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    }
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);

  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;

  // Cria o editor uma vez.
  // biome-ignore lint/correctness/useExhaustiveDependencies: o editor é criado uma vez de propósito
  useEffect(() => {
    if (!editorRef.current) return;
    const view = new EditorView({
      state: EditorState.create({
        doc: "",
        extensions: [
          basicSetup,
          oneDark,
          langCompartment.current.of([]),
          keymap.of([
            {
              key: "Mod-s",
              run: () => {
                void save();
                return true;
              },
            },
          ]),
          EditorView.updateListener.of((u: ViewUpdate) => {
            if (u.docChanged && !loadingRef.current) {
              setContent(u.state.doc.toString());
              setDirty(true);
            }
          }),
        ],
      }),
      parent: editorRef.current,
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  async function refreshFiles() {
    if (!hub || !profile || !project) return;
    try {
      const res = await hub.listFiles(profile, project);
      setFiles(res.files);
      const folders = new Set<string>();
      for (const f of res.files) {
        const parts = f.split("/");
        for (let i = 1; i < parts.length; i++) {
          folders.add(parts.slice(0, i).join("/"));
        }
      }
      setExpanded(folders);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    }
  }

  async function refreshStatus() {
    if (!hub || !profile || !project) return;
    try {
      const res = await hub.gitStatus(profile, project);
      setStatus(res.files);
    } catch {
      // silencioso
    }
  }

  // biome-ignore lint/correctness/useExhaustiveDependencies: recarrega quando muda de perfil/projeto
  useEffect(() => {
    void refreshFiles();
    void refreshStatus();
  }, [profile, project, hub]);

  async function loadFile(path: string) {
    if (!hub || !profile || !project) return;
    setLoading(true);
    loadingRef.current = true;
    try {
      const res = await hub.readFile(profile, project, path);
      setContent(res.content);
      setDirty(false);
      const v = viewRef.current;
      if (v) {
        v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: res.content } });
        const desc = LanguageDescription.matchFilename(languages, path);
        if (desc) {
          try {
            const lang = await desc.load();
            v.dispatch({ effects: langCompartment.current.reconfigure(lang) });
          } catch {
            // linguagem indisponível: segue sem destaque
          }
        } else {
          v.dispatch({ effects: langCompartment.current.reconfigure([]) });
        }
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    } finally {
      // libera após o dispatch ser processado
      setTimeout(() => {
        loadingRef.current = false;
      }, 0);
      setLoading(false);
    }
  }

  function selectFile(path: string) {
    if (dirtyRef.current && selected && path !== selected) {
      if (!window.confirm("Descartar alterações não salvas?")) return;
    }
    setSelected(path);
    setMode("editor");
  }

  async function save() {
    if (!selected || !hub || !profile || !project) return;
    const text = viewRef.current?.state.doc.toString() ?? content;
    setSaving(true);
    try {
      await hub.writeFile(profile, project, selected, text);
      setContent(text);
      setDirty(false);
      await refreshStatus();
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  // biome-ignore lint/correctness/useExhaustiveDependencies: carrega o arquivo quando a seleção muda
  useEffect(() => {
    if (selected) void loadFile(selected);
  }, [selected]);

  function openDiff(path: string) {
    setDiffTarget(path);
    setMode("diff");
  }

  async function discardFile(path: string) {
    if (!hub || !profile || !project) return;
    if (!window.confirm(`Descartar todas as alterações em ${path}?`)) return;
    try {
      const res = await hub.gitDiff(profile, project, path);
      const head = res.head ?? "";
      await hub.writeFile(profile, project, path, head);
      if (selected === path) {
        setContent(head);
        setDirty(false);
        const v = viewRef.current;
        if (v) {
          loadingRef.current = true;
          v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: head } });
          setTimeout(() => {
            loadingRef.current = false;
          }, 0);
        }
      }
      await refreshStatus();
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    }
  }

  if (!profile || !project) {
    return <div className="p-6 text-slate-400">Escolha um projeto para editar.</div>;
  }

  return (
    <div ref={containerRef} className="flex h-full min-h-0">
      {/* Lateral: arquivos */}
      <aside style={{ width: leftWidth }} className="flex shrink-0 flex-col border-r border-line">
        <header className="flex items-center justify-between border-b border-line px-3 py-2">
          <span className="text-sm font-semibold text-slate-200">Arquivos</span>
          <button
            type="button"
            onClick={() => {
              void refreshFiles();
              void refreshStatus();
            }}
            className="rounded p-1 text-slate-400 hover:text-slate-200"
            title="Atualizar"
          >
            <RefreshCw className="size-3.5" />
          </button>
        </header>
        <div className="flex-1 overflow-auto p-1">
          {files.length === 0 && (
            <p className="px-2 py-4 text-xs text-slate-500">Nenhum arquivo.</p>
          )}
          <FileTree
            files={files}
            status={status}
            selected={selected}
            expanded={expanded}
            onToggle={(folder) =>
              setExpanded((prev) => {
                const next = new Set(prev);
                if (next.has(folder)) next.delete(folder);
                else next.add(folder);
                return next;
              })
            }
            onSelect={selectFile}
          />
        </div>
        {/* Alterados */}
        {status.length > 0 && (
          <div className="border-t border-line">
            <header className="px-3 py-1.5 text-xs font-semibold text-slate-400">
              Alterados ({status.length})
            </header>
            <ul className="max-h-40 overflow-auto p-1">
              {status.map((s) => (
                <li key={s.path} className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={() => {
                      selectFile(s.path);
                    }}
                    className="flex-1 truncate px-2 py-0.5 text-left text-xs font-mono text-slate-300 hover:bg-white/5"
                  >
                    {s.path}
                  </button>
                  <button
                    type="button"
                    title="Ver diff"
                    onClick={() => openDiff(s.path)}
                    className="rounded p-0.5 text-slate-400 hover:text-slate-200"
                  >
                    <GitCompare className="size-3.5" />
                  </button>
                  <button
                    type="button"
                    title="Descartar alterações"
                    onClick={() => discardFile(s.path)}
                    className="rounded p-0.5 text-slate-400 hover:text-rose-300"
                  >
                    <X className="size-3.5" />
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
      </aside>

      <button
        type="button"
        aria-label="Redimensionar a lateral"
        onMouseDown={() => {
          draggingRef.current = true;
          document.body.style.cursor = "col-resize";
          document.body.style.userSelect = "none";
        }}
        className="w-1 shrink-0 cursor-col-resize border-0 bg-line/60 p-0 hover:bg-accent/60"
        title="Arraste para redimensionar"
      />

      {/* Área principal */}
      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-2 border-b border-line px-4 py-2">
          <span className="truncate font-mono text-sm text-slate-200">
            {selected ?? "Selecione um arquivo"}
          </span>
          {dirty && <span className="text-xs text-amber-300">● não salvo</span>}
          <div className="ml-auto flex gap-2">
            {selected && (
              <>
                <button
                  type="button"
                  onClick={() => openDiff(selected)}
                  className="flex items-center gap-1 rounded-lg border border-line bg-white/5 px-3 py-1 text-xs text-slate-200 hover:bg-white/10"
                >
                  <GitCompare className="size-3.5" /> Diff
                </button>
                <button
                  type="button"
                  onClick={() => {
                    void save();
                  }}
                  disabled={saving || !dirty}
                  className="flex items-center gap-1 rounded-lg bg-accent px-3 py-1 text-xs font-medium text-white disabled:opacity-50"
                >
                  <Save className="size-3.5" /> {saving ? "Salvando…" : "Salvar"}
                </button>
              </>
            )}
            {onClose && (
              <button
                type="button"
                onClick={onClose}
                title="Fechar o editor"
                className="flex items-center gap-1 rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-1 text-xs font-medium text-rose-200 hover:bg-rose-500/20"
              >
                <X className="size-3.5" /> Fechar
              </button>
            )}
            {mode === "diff" && (
              <button
                type="button"
                onClick={() => setMode("editor")}
                className="rounded-lg border border-line bg-white/5 px-3 py-1 text-xs text-slate-200 hover:bg-white/10"
              >
                Voltar ao editor
              </button>
            )}
          </div>
        </header>
        {mode === "editor" ? (
          <div ref={editorRef} className="min-h-0 flex-1 overflow-hidden bg-[#282c34]" />
        ) : (
          <DiffView profile={profile} project={project} path={diffTarget ?? selected ?? ""} />
        )}
        {loading && (
          <div className="border-t border-line px-4 py-1 text-xs text-slate-500">Carregando…</div>
        )}
      </section>
    </div>
  );
}
