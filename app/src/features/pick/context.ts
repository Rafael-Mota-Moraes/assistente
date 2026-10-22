/** Contexto de código do pick, do lado do app (F12.3).
 *
 * A página do preview entrega o arquivo e a linha do JSX em dev (Vite + React). Aqui isso
 * vira o caminho do projeto (`/src/features/x/Botao.tsx` → `src/features/x/Botao.tsx`) e vira
 * texto para o pedido: em dev, "onde" é o arquivo:linha; em build de produção não há fonte,
 * então o pick leva componente, seletor e texto, e o pedido manda o agente procurar o arquivo
 * antes de editar em vez de chutar. */
import type { PickCodeContext, PickElement } from "./types";

/** Fonte do elemento apontado, já no caminho do projeto. */
export type ResolvedSource = {
  /** Caminho dentro do projeto (ex.: `src/features/x/Botao.tsx`). */
  path: string;
  /** Linha do JSX (null se o build não expõe). */
  line: number | null;
  /** Coluna do JSX (null se o build não expõe). */
  column: number | null;
  /** `src/features/x/Botao.tsx:42` (sem linha: só o caminho). */
  label: string;
};

/** Contexto resolvido: o que o app mostra e o que vai no pedido. */
export type ResolvedCodeContext = {
  /** A página usa React? */
  react: boolean;
  /** Componente mais próximo do elemento. */
  component: string | null;
  /** Pilha de componentes, do mais interno ao mais externo. */
  stack: string[];
  /** Arquivo:linha no projeto (null em build de produção). */
  source: ResolvedSource | null;
};

/**
 * Caminho do arquivo dentro do projeto.
 *
 * O dev server do Vite serve o projeto na raiz, então o `_debugSource` vem como `/src/…` (ou
 * com a URL do dev server na frente). Caminho absoluto do projeto vira relativo; sem raiz
 * conhecida, o `/src/…` ainda vira `src/…` — que é o que o agente precisa para achar o
 * arquivo com grep.
 */
export function toProjectPath(file: string, projectRoot?: string | null): string {
  const trimmed = file.trim();
  if (!trimmed) return "";
  let path = trimmed;
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(path)) {
    try {
      path = new URL(path).pathname;
    } catch {
      // URL malformada: fica como está.
    }
  }
  path = path.replace(/\\/g, "/");
  const root = projectRoot?.replace(/\\/g, "/").replace(/\/+$/, "");
  if (root) {
    if (path.startsWith(`${root}/`)) return path.slice(root.length + 1);
  }
  const marker = path.indexOf("/src/");
  if (marker >= 0) return path.slice(marker + 1);
  if (path.startsWith("src/")) return path;
  return path.replace(/^\.\//, "");
}

/** Resolve o contexto que veio da página: caminho do projeto e texto de "onde está". */
export function resolveCodeContext(
  code: PickCodeContext | null | undefined,
  projectRoot?: string | null,
): ResolvedCodeContext {
  const stack = code?.stack ?? [];
  const component = code?.component ?? null;
  const react = code?.react ?? false;
  const raw = code?.source;
  if (!raw?.file) return { react, component, stack, source: null };
  const path = toProjectPath(raw.file, projectRoot);
  const label = raw.line === null ? path : `${path}:${raw.line}`;
  return { react, component, stack, source: { path, line: raw.line, column: raw.column, label } };
}

/** Componente e pilha, sem o arquivo (a tela mostra o arquivo separado). */
export function describeComponents(context: ResolvedCodeContext): string {
  const parts: string[] = [];
  if (context.component) parts.push(`componente ${context.component}`);
  if (context.stack.length > 1) parts.push(`pilha: ${context.stack.join(" > ")}`);
  return parts.join(" · ");
}

/** "Onde está" o elemento: arquivo:linha em dev; componente, seletor e texto sem fonte. */
export function describeWhere(context: ResolvedCodeContext, pick: PickElement): string {
  const parts: string[] = [];
  if (context.source) parts.push(context.source.label);
  const components = describeComponents(context);
  if (components) parts.push(components);
  if (!context.source) {
    parts.push(`seletor ${pick.selector}`);
    if (pick.text) parts.push(`texto "${pick.text}"`);
  }
  return parts.join(" · ");
}

/** Pistas para o agente achar o arquivo quando a página não expõe a fonte. */
export function searchHints(pick: PickElement, context: ResolvedCodeContext): string[] {
  const hints: string[] = [];
  if (pick.text) hints.push(`o texto "${pick.text}"`);
  // Classe que o próprio apontar põe no elemento não ajuda a achar o arquivo.
  const classes = (pick.classes ?? []).filter((c) => c && !c.startsWith("assistente-"));
  if (classes.length > 0) hints.push(`a classe "${classes[0]}"`);
  if (context.component) hints.push(`o componente "${context.component}"`);
  return hints;
}

/**
 * Aviso que vai no pedido quando não há arquivo de origem (build de produção): o agente
 * procura antes de editar, em vez de chutar o caminho.
 */
export function searchInstruction(pick: PickElement, context: ResolvedCodeContext): string {
  const hints = searchHints(pick, context);
  const how = hints.length > 0 ? `grep por ${hints.join(", por ")}` : "grep pelo seletor";
  const motivo = context.react
    ? "Sem arquivo de origem (build de produção)"
    : "A página não expõe o código do elemento";
  return (
    `${motivo}: antes de editar, procure o arquivo no projeto ` +
    `(${how}) e confirme o caminho; não chute o arquivo.`
  );
}
