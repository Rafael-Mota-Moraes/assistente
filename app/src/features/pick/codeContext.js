/**
 * Contexto de código do elemento apontado (F12.3).
 *
 * Script clássico (sem import/export) porque ele é injetado na página do preview junto com o
 * script do modo apontar (F12.2). O script do apontar chama:
 *
 *   const code = window.__assistenteCodeContext.extract(element);
 *
 * e manda o resultado no payload do pick (campo `code`).
 *
 * Em dev (Vite + React) o React deixa o `_debugSource` no fiber: dá para dizer de qual arquivo
 * e linha veio o elemento. Em build de produção não há fonte: aí sobram o componente e a pilha,
 * e o agente procura o arquivo pelo texto/classe/componente antes de editar.
 */
(() => {
  /** Chave que o React deixa no DOM: `__reactFiber$…` (React 17+) ou `__reactInternalInstance$…`. */
  function fiberKey(node) {
    const keys = Object.keys(node);
    for (const key of keys) {
      if (key.startsWith("__reactFiber$") || key.startsWith("__reactInternalInstance$")) {
        return key;
      }
    }
    return null;
  }

  /** Nome do componente do fiber (null para elemento do DOM e para tipos sem nome). */
  function componentName(fiber) {
    return typeName(fiber.elementType || fiber.type);
  }

  function typeName(type) {
    if (typeof type === "string") return null; // elemento do DOM ("button")
    if (typeof type === "function") return type.displayName || type.name || null;
    if (type && typeof type === "object") {
      // memo()/forwardRef(): o nome do componente de verdade está dentro.
      if (typeof type.type === "function") return type.type.displayName || type.type.name || null;
      if (typeof type.render === "function")
        return type.render.displayName || type.render.name || null;
    }
    return null;
  }

  /** `arquivo:linha` do JSX, quando o build expõe (só em dev). */
  function debugSource(fiber) {
    const candidates = [
      fiber._debugSource,
      propsSource(fiber.memoizedProps),
      propsSource(fiber.pendingProps),
    ];
    for (const candidate of candidates) {
      if (candidate && typeof candidate.fileName === "string" && candidate.fileName !== "") {
        return {
          file: candidate.fileName,
          line: numberOrNull(candidate.lineNumber),
          column: numberOrNull(candidate.columnNumber),
        };
      }
    }
    return null;
  }

  /** React 17 e anteriores: o `__source` ia nas props do elemento. */
  function propsSource(props) {
    if (!props || typeof props !== "object") return null;
    return props.__source || null;
  }

  function numberOrNull(value) {
    return typeof value === "number" && Number.isFinite(value) ? value : null;
  }

  /**
   * Contexto de código do elemento: sobe pelo fiber do React e devolve o componente mais
   * próximo, a pilha de componentes e (em dev) o arquivo:linha do JSX.
   *
   * @param {Element | null | undefined} element elemento apontado na página
   * @returns {{react: boolean, component: string | null, stack: string[], source: ({file: string, line: number | null, column: number | null} | null)}}
   */
  function extract(element) {
    const empty = { react: false, component: null, stack: [], source: null };
    if (!element || typeof element !== "object") return empty;
    const key = fiberKey(element);
    if (!key) return empty; // página sem React (ou fora do React): nada a dizer sobre o código.

    const stack = [];
    let source = null;
    let fiber = element[key];
    // A pilha é curta (dezenas de níveis); o limite é só para não girar num fiber quebrado.
    for (let depth = 0; fiber && depth < 500; depth += 1) {
      const name = componentName(fiber);
      if (name && stack[stack.length - 1] !== name) stack.push(name);
      // O primeiro `_debugSource` é o do elemento apontado (o do JSX que o criou).
      if (!source) source = debugSource(fiber);
      fiber = fiber.return;
    }
    return { react: true, component: stack.length > 0 ? stack[0] : null, stack, source };
  }

  globalThis.__assistenteCodeContext = { extract };
})();
