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
