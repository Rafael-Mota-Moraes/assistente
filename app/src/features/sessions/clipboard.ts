// Copiar/colar do terminal. Ctrl+C e Ctrl+V continuam indo para o agente (interromper, colar
// imagem), como num terminal Linux: a área de transferência fica no Ctrl+Shift.
import { isTauri } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

export type ClipboardAction = "copy" | "paste";

export function clipboardShortcut(event: KeyboardEvent): ClipboardAction | null {
  if (!event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey) return null;
  const key = event.key.toLowerCase();
  if (key === "c") return "copy";
  if (key === "v") return "paste";
  return null;
}

// No app desktop vai pelo plugin do Tauri: o navigator.clipboard do WebKitGTK falha em silêncio.
export function copyText(text: string): Promise<void> {
  return isTauri() ? writeText(text) : navigator.clipboard.writeText(text);
}

export function pasteText(): Promise<string> {
  return isTauri() ? readText() : navigator.clipboard.readText();
}

/**
 * Texto de um OSC 52 (`<seleção>;<base64>`), que o tmux envia ao copiar com o mouse. Pedidos de
 * leitura (`?`) e conteúdo inválido dão null: o agente pode escrever na área de transferência,
 * nunca ler.
 */
export function osc52Text(data: string): string | null {
  const payload = data.slice(data.indexOf(";") + 1);
  if (!data.includes(";") || !payload || payload === "?") return null;
  try {
    const bytes = Uint8Array.from(atob(payload), (c) => c.charCodeAt(0));
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/**
 * O xterm.js só esvazia a textarea escondida no Enter e no Ctrl+C. No WebKitGTK as teclas se
 * acumulam nela, e no fim de cada acento o xterm.js manda a textarea a partir de uma posição velha:
 * `a á ã` chegava como `a á  á ã`. Esvazia a textarea a cada tecla fora de composição, antes do
 * xterm.js ler (captura no `host`), e espera ele ler o acento composto (o `setTimeout` do
 * `compositionend`). Chamar depois do `term.open`, para os ouvintes do xterm.js virem antes.
 */
export function clearStaleTextarea(host: HTMLElement, textarea: HTMLTextAreaElement) {
  let composing = false;
  const onStart = () => {
    composing = true;
  };
  const onEnd = () => {
    setTimeout(() => {
      composing = false;
    }, 0);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (!composing && !event.isComposing && event.target === textarea) textarea.value = "";
  };
  textarea.addEventListener("compositionstart", onStart);
  textarea.addEventListener("compositionend", onEnd);
  host.addEventListener("keydown", onKeyDown, true);
  return () => {
    textarea.removeEventListener("compositionstart", onStart);
    textarea.removeEventListener("compositionend", onEnd);
    host.removeEventListener("keydown", onKeyDown, true);
  };
}

/**
 * Filtro da entrada do xterm.js: no WebKitGTK um acento morto (~ + a) chega duas vezes, pela
 * composição e pelo evento `input`. Descarta o mesmo texto não-ASCII repetido em poucos ms,
 * rápido demais para ter sido digitado de novo.
 */
export function dedupeComposedInput(windowMs = 50, now = () => performance.now()) {
  let last = "";
  let lastAt = Number.NEGATIVE_INFINITY;
  return (data: string): boolean => {
    const at = now();
    const nonAscii = [...data].some((c) => c.charCodeAt(0) > 0x7f);
    const repeated = nonAscii && data === last && at - lastAt < windowMs;
    last = data;
    lastAt = at;
    return !repeated;
  };
}
