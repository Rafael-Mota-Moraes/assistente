// Modo apontar (F12.2). Este script é injetado na webview do preview (fica fora do
// bundle do projeto): um botão flutuante liga o modo, o elemento sob o mouse ganha
// contorno e o clique é capturado na fase de captura — a página não recebe o clique.
// O pick sai por uma navegação `assistente-pick:...`: sem IPC e sem CORS na página
// remota. O Rust intercepta essa navegação e entrega o pick para o app.
(() => {
  "use strict";

  const API_KEY = "__assistentePick";
  const SCHEME = "assistente-pick:";
  const MAX_TEXT = 160;
  const MAX_HTML = 600;
  const MAX_CLASSES = 12;
  const MAX_ARIA = 8;

  // Webviews que não são alvo de apontar: a UI do app e o painel da memória.
  const IGNORED_LABELS = ["main", "memoria"];

  // O rótulo da webview vem do app (o Rust injeta o `__TAURI_INTERNALS__`).
  // Sem rótulo não dá para saber se esta webview é alvo de apontar: o botão
  // flutuante não aparece (a dúvida vale mais que o botão sobrando).
  const internals = window.__TAURI_INTERNALS__;
  const metadata = internals ? internals.metadata : null;
  const current = metadata ? metadata.currentWebview : null;
  const label = current ? current.label : null;
  if (!label) return;
  if (IGNORED_LABELS.indexOf(label) >= 0) return;
  if (window[API_KEY]) return;

  const state = {
    enabled: false,
    hovered: null,
    button: null,
    outline: null,
    frame: 0,
  };

  function cssEscape(value) {
    if (window.CSS && typeof window.CSS.escape === "function") return window.CSS.escape(value);
    return String(value).replace(/[^a-zA-Z0-9_-]/g, "\\$&");
  }

  function shorten(value, max) {
    const text = String(value === null || value === undefined ? "" : value)
      .replace(/\s+/g, " ")
      .trim();
    if (!text) return null;
    return text.length > max ? `${text.slice(0, max - 1)}…` : text;
  }

  function round(value) {
    return Math.round((Number(value) || 0) * 100) / 100;
  }

  function tagOf(element) {
    return element && element.tagName ? element.tagName.toLowerCase() : "";
  }

  /** Caminho do elemento no DOM (`body > main > button:nth-child(2)`). */
  function domPath(element) {
    const parts = [];
    let node = element;
    while (node && node.nodeType === 1 && parts.length < 12) {
      let part = tagOf(node);
      if (node.id) {
        parts.unshift(`${part}#${cssEscape(node.id)}`);
        break;
      }
      const parent = node.parentElement;
      if (parent) {
        const index = Array.prototype.indexOf.call(parent.children, node) + 1;
        part += `:nth-child(${index})`;
      }
      parts.unshift(part);
      node = parent;
    }
    return parts.join(" > ");
  }

  /** Seletor CSS estável: id, classes ou (nada melhor) o caminho. */
  function selectorOf(element) {
    if (element.id) return `#${cssEscape(element.id)}`;
    const tag = tagOf(element);
    const classes = [];
    for (const name of element.classList) {
      if (classes.length >= 3) break;
      classes.push(`.${cssEscape(name)}`);
    }
    if (classes.length) return `${tag}${classes.join("")}`;
    return domPath(element);
  }

  function ariaOf(element) {
    const out = {};
    const attributes = element.attributes || [];
    for (const attribute of attributes) {
      if (attribute.name.indexOf("aria-") !== 0) continue;
      out[attribute.name.slice(5)] = attribute.value;
      if (Object.keys(out).length >= MAX_ARIA) break;
    }
    return out;
  }

  /** O pick: o que o agente precisa saber do elemento escolhido. */
  function describe(element) {
    const rect = element.getBoundingClientRect();
    return {
      tag: tagOf(element),
      id: element.id || null,
      classes: Array.prototype.slice.call(element.classList, 0, MAX_CLASSES),
      role: element.getAttribute("role"),
      aria: ariaOf(element),
      text: shorten(element.innerText || element.textContent || "", MAX_TEXT),
      html: shorten(element.outerHTML || "", MAX_HTML),
      selector: selectorOf(element),
      dom_path: domPath(element),
      rect: {
        x: round(rect.x),
        y: round(rect.y),
        width: round(rect.width),
        height: round(rect.height),
      },
      viewport: { width: round(window.innerWidth), height: round(window.innerHeight) },
      scroll: { x: round(window.scrollX), y: round(window.scrollY) },
      url: location.href,
    };
  }

  function whenReady(callback) {
    if (document.body) {
      callback();
      return;
    }
    document.addEventListener("DOMContentLoaded", callback, { once: true });
  }

  function ensureOutline() {
    if (state.outline) return state.outline;
    const box = document.createElement("div");
    box.setAttribute("data-assistente-pick", "outline");
    box.style.position = "fixed";
    box.style.pointerEvents = "none";
    box.style.zIndex = "2147483646";
    box.style.border = "2px solid rgb(167 139 250)";
    box.style.background = "rgba(167 139 250, 0.12)";
    box.style.borderRadius = "3px";
    box.style.display = "none";
    document.body.appendChild(box);
    state.outline = box;
    return box;
  }

  function ensureButton() {
    if (state.button) return state.button;
    const button = document.createElement("button");
    button.type = "button";
    button.setAttribute("data-assistente-pick", "button");
    button.textContent = "Apontar";
    button.style.position = "fixed";
    button.style.top = "12px";
    button.style.right = "12px";
    button.style.zIndex = "2147483647";
    button.style.padding = "6px 12px";
    button.style.font = "600 13px/1.2 system-ui, sans-serif";
    button.style.color = "#0f172a";
    button.style.background = "#a78bfa";
    button.style.border = "none";
    button.style.borderRadius = "999px";
    button.style.cursor = "pointer";
    button.style.boxShadow = "0 4px 14px rgba(0, 0, 0, 0.35)";
    button.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      toggle();
    });
    document.body.appendChild(button);
    state.button = button;
    return button;
  }

  function hideOutline() {
    if (state.outline) state.outline.style.display = "none";
    state.hovered = null;
  }

  function highlight(x, y) {
    if (typeof document.elementFromPoint !== "function") return;
    const element = document.elementFromPoint(x, y);
    if (!element) {
      hideOutline();
      return;
    }
    const box = ensureOutline();
    const rect = element.getBoundingClientRect();
    box.style.display = "block";
    box.style.left = `${rect.left}px`;
    box.style.top = `${rect.top}px`;
    box.style.width = `${rect.width}px`;
    box.style.height = `${rect.height}px`;
    state.hovered = element;
  }

  function onMove(event) {
    if (!state.enabled || state.frame) return;
    const x = event.clientX;
    const y = event.clientY;
    state.frame = requestAnimationFrame(() => {
      state.frame = 0;
      highlight(x, y);
    });
  }

  function onClick(event) {
    if (!state.enabled) return;
    const target = event.target;
    // O botão e o contorno são do app: não são alvo de apontar.
    if (target && target.closest && target.closest("[data-assistente-pick]")) return;
    // A fase de captura: o clique não chega no elemento, a página não age.
    event.preventDefault();
    event.stopPropagation();
    if (typeof event.stopImmediatePropagation === "function") event.stopImmediatePropagation();
    const element = target && target.nodeType === 1 ? target : null;
    if (!element) return;
    api.send(describe(element));
  }

  function onKey(event) {
    if (event.key !== "Escape" || !state.enabled) return;
    event.preventDefault();
    event.stopPropagation();
    disable();
  }

  function enable() {
    if (state.enabled) return;
    state.enabled = true;
    document.addEventListener("click", onClick, true);
    document.addEventListener("mousemove", onMove, true);
    document.addEventListener("keydown", onKey, true);
    whenReady(() => {
      const button = ensureButton();
      button.textContent = "Apontando… (Esc cancela)";
    });
  }

  function disable() {
    if (!state.enabled) return;
    state.enabled = false;
    document.removeEventListener("click", onClick, true);
    document.removeEventListener("mousemove", onMove, true);
    document.removeEventListener("keydown", onKey, true);
    hideOutline();
    whenReady(() => {
      if (state.button) state.button.textContent = "Apontar";
    });
  }

  function toggle() {
    if (state.enabled) disable();
    else enable();
  }

  function remove() {
    disable();
    if (state.button && state.button.parentNode) state.button.parentNode.removeChild(state.button);
    if (state.outline && state.outline.parentNode) state.outline.parentNode.removeChild(state.outline);
    state.button = null;
    state.outline = null;
  }

  const api = {
    enable,
    disable,
    toggle,
    /** O botão "apontar" da barra do preview (F12.1) liga e desliga por aqui (`preview_picker`). */
    setEnabled(on) {
      if (on) enable();
      else disable();
    },
    isEnabled: () => state.enabled,
    remove,
    /** Último pick enviado (para depurar). */
    last: null,
    /** Envia o pick: monta a URL e navega. */
    send(payload) {
      const url = `${SCHEME}${encodeURIComponent(JSON.stringify(payload))}`;
      api.last = payload;
      document.dispatchEvent(
        new CustomEvent("assistente-pick", { detail: { url, payload } }),
      );
      api.navigate(url);
    },
    /** Como o pick sai da página. Trocar aqui troca o destino (o app e os testes usam). */
    navigate(url) {
      window.location.href = url;
    },
  };

  window[API_KEY] = api;
  whenReady(() => {
    ensureButton();
  });
})();
