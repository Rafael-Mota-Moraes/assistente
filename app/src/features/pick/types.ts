/** Contexto de código do elemento apontado (F12.3).
 *
 * Colhido na página do preview: o script `codeContext.js` sobe pelo fiber do React
 * (`__reactFiber$…`) e devolve o componente mais próximo, a pilha de componentes e — em dev
 * (Vite + React) — o arquivo e a linha do JSX (`_debugSource` no React 18 e anteriores,
 * `_debugStack` no React 19). Em build de produção não há fonte: aí sobram o componente e a
 * pilha, e o agente procura o arquivo pelo texto/classe/componente antes de editar. */
export type PickCodeContext = {
  /** A página usa React? */
  react: boolean;
  /** Componente mais próximo do elemento (null se a página não usa React). */
  component: string | null;
  /** Pilha de componentes, do mais interno ao mais externo. */
  stack: string[];
  /** Arquivo e linha do JSX (null em build de produção). */
  source: { file: string; line: number | null; column: number | null } | null;
};

/** O que o modo apontar (F12.2) entrega ao app, mais o contexto de código (F12.3). */
export type PickElement = {
  /** URL real da página (o Rust anexa, não confia no payload). */
  url: string;
  /** Tag do elemento (`button`, `a`, `h1`…). */
  tag: string;
  /** Texto curto do elemento. */
  text: string;
  /** Seletor CSS estável do elemento. */
  selector: string;
  /** Classes do elemento. */
  classes?: string[];
  /** Contexto de código (F12.3). */
  code?: PickCodeContext | null;
};
