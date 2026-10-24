/** Tipos do script de contexto de código injetado na página do preview (F12.3).
 *
 * O script (`codeContext.js`) é um script clássico: ele deixa a API em
 * `globalThis.__assistenteCodeContext` para o script do modo apontar (F12.2) chamar. */

import type { PickCodeContext } from "../../bridge/types";

/** Contexto de código de um elemento apontado: o mesmo tipo do pick (`PickedElement.code`). */
export type CodeContext = PickCodeContext;

/** API que o script deixa na página. */
export type CodeContextApi = {
  extract: (element: Element | null | undefined) => CodeContext;
};

declare global {
  /** Deixado pelo `codeContext.js` na página do preview. */
  var __assistenteCodeContext: CodeContextApi | undefined;
}
