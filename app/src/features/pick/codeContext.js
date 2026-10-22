/**
 * Contexto de código do elemento apontado (F12.3).
 *
 * Script clássico (sem import/export) porque ele é injetado na página do preview junto com o
 * script do modo apontar (F12.2): o Rust o include com `include_str!` e passa na
 * `initialization_script` da webview — antes do script do apontar.
 *
 * O script do apontar chama a API que este arquivo deixa na página:
 *
 *   const code = window.__assistenteCodeContext.extract(element);
 *
 * e manda o resultado no payload do pick (campo `code`).
 *
 * De onde sai o "arquivo:linha":
 *
 * - React 18 e anteriores: o `_debugSource` do fiber (o `__source` do JSX).
 * - React 19+: o `_debugSource` sumiu; o React põe um Error no `_debugStack` do fiber, criado
 *   no momento do JSX — a pilha dele tem o arquivo e a linha (o frame do próprio React vem
 *   antes e é pulado).
 *
 * Em build de produção não há fonte: aí sobram o componente e a pilha, e o agente procura o
 * arquivo pelo texto/classe/componente antes de editar.
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
      if (typeof type.render === "function") {
        return type.render.displayName || type.render.name || null;
      }
    }
    return null;
  }

  /** `arquivo:linha` do JSX, quando o build expõe (só em dev). */
  function debugSource(fiber) {
    // React 18 e anteriores: o `_debugSource` do fiber.
    const direct = fiber._debugSource;
    if (direct && typeof direct.fileName === "string" && direct.fileName !== "") {
      return {
        file: direct.fileName,
        line: numberOrNull(direct.lineNumber),
        column: numberOrNull(direct.columnNumber),
      };
    }
    // React 17 e anteriores: o `__source` ia nas props do elemento.
    const props = propsSource(fiber.memoizedProps) || propsSource(fiber.pendingProps);
    if (props && typeof props.fileName === "string" && props.fileName !== "") {
      return {
        file: props.fileName,
        line: numberOrNull(props.lineNumber),
        column: numberOrNull(props.columnNumber),
      };
    }
    // React 19+: o `_debugStack` (Error criado no JSX) tem o lugar do JSX na pilha.
    return stackSource(fiber._debugStack);
  }

  /** React 17 e anteriores: o `__source` ia nas props do elemento. */
  function propsSource(props) {
    if (!props || typeof props !== "object") return null;
    return props.__source || null;
  }

  /** React 19+: o primeiro frame que não é do próprio React é o do JSX. */
  function stackSource(error) {
    if (!error || typeof error.stack !== "string") return null;
    const lines = error.stack.split("\n");
    for (const line of lines) {
      const location = frameLocation(line);
      if (!location) continue;
      if (isReactInternal(location.file)) continue;
      return location;
    }
    return null;
  }

  /** "at Botao (/caminho/Botao.tsx:42:17)" → {file, line, column}. */
  function frameLocation(line) {
    const match = line.match(/([^\s()]+?):(\d+):(\d+)/);
    if (!match) return null;
    let file = match[1];
    // Firefox e Safari: "Botao@http://localhost:5173/src/x.tsx:42:17".
    const at = file.lastIndexOf("@");
    if (at > 0 && !file.slice(0, at).includes("/") && !file.slice(0, at).includes(":")) {
      file = file.slice(at + 1);
    }
    return { file, line: Number(match[2]), column: Number(match[3]) };
  }

  /** O frame é do próprio React (o `jsxDEV` e o react-dom)? Aí não é o arquivo do usuário. */
  function isReactInternal(file) {
    if (file.includes("node_modules")) return true;
    if (file.includes("react-dom") || file.includes("react_jsx")) return true;
    if (file.includes("react.development") || file.includes("react.production")) return true;
    // Um arquivo JS dentro de uma pasta chamada "react" (o build do próprio React).
    return /[/\\]react[/\\][^/\\]*\.(c|m)?js$/i.test(file);
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
      // O primeiro frame com fonte é o do elemento apontado (o do JSX que o criou).
      if (!source) source = debugSource(fiber);
      fiber = fiber.return;
    }
    return { react: true, component: stack.length > 0 ? stack[0] : null, stack, source };
  }

  globalThis.__assistenteCodeContext = { extract };
})();
