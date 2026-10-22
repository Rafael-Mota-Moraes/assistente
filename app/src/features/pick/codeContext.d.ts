/** Tipos do script de contexto de código injetado na página do preview (F12.3).
 *
 * O script (`codeContext.js`) é um script clássico: ele deixa a API em
 * `globalThis.__assistenteCodeContext` para o script do modo apontar (F12.2) chamar. */

/** Arquivo e linha do JSX (só em dev; em build de produção não existe). */
export type CodeContextSource = {
  file: string;
  line: number | null;
  column: number | null;
};

/** Contexto de código de um elemento apontado. */
export type CodeContext = {
  /** A página usa React? */
  react: boolean;
  /** Componente mais próximo do elemento. */
  component: string | null;
  /** Pilha de componentes, do mais interno ao mais externo. */
  stack: string[];
  /** Arquivo e linha do JSX (null sem dev). */
  source: CodeContextSource | null;
};

/** API que o script deixa na página. */
export type CodeContextApi = {
  extract: (element: Element | null | undefined) => CodeContext;
};

declare global {
  /** Deixado pelo `codeContext.js` na página do preview. */
  var __assistenteCodeContext: CodeContextApi | undefined;
}
