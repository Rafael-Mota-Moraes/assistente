// Comandos que só existem no app desktop (app/src-tauri/src/main.rs). No navegador
// (pnpm dev sem Tauri) caem em valores neutros para a UI continuar navegável.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import type { HubConnection, VmStatus } from "./types";

const DEV_HUB_URL = "http://127.0.0.1:7878";

/** No app, o fetch passa pelo Rust (plugin http): o WebKit bloqueia http:// a partir de
 * tauri://localhost. Mudou o IP ou a porta da VM? Atualize capabilities/default.json. */
export const hubFetch: typeof fetch = (...args) =>
  isTauri() ? tauriFetch(...args) : fetch(...args);

export function hubConnection(): Promise<HubConnection> {
  if (!isTauri()) return Promise.resolve({ url: DEV_HUB_URL, token: null });
  return invoke<HubConnection>("hub_connection");
}

export function vmStatus(): Promise<VmStatus> {
  if (!isTauri()) return Promise.resolve({ state: "unknown", detail: "fora do app desktop" });
  return invoke<VmStatus>("vm_status");
}

export function vmStart(): Promise<void> {
  if (!isTauri()) return Promise.reject(new Error("só funciona no app desktop"));
  return invoke("vm_start");
}

export function vmStop(): Promise<void> {
  if (!isTauri()) return Promise.reject(new Error("só funciona no app desktop"));
  return invoke("vm_stop");
}

export function openInCursor(path: string): Promise<void> {
  if (!isTauri()) return Promise.reject(new Error("só funciona no app desktop"));
  return invoke("open_in_cursor", { path });
}

/** Retângulo do âncora do painel em pixels lógicos (igual ao px do CSS). */
export type PanelRect = { x: number; y: number; width: number; height: number };

/** Webview da memória dentro da janela principal (F5.5): no app é uma webview filha ancorada
 * sobre a área de conteúdo; no navegador (pnpm dev), o painel usa iframe e aqui não faz nada. */
export function openMemoryPanel(url: string, rect: PanelRect): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke("open_memory_panel", { url, rect });
}

/** Acompanha o âncora do painel (janela redimensionou, layout mudou). */
export function updateMemoryPanelBounds(rect: PanelRect): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke("update_memory_panel_bounds", { rect });
}

/** Fecha a webview da memória. */
export function closeMemoryPanel(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke("close_memory_panel");
}

/** Abre uma URL no navegador padrão (links clicáveis do terminal). */
export function openUrl(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener");
    return Promise.resolve();
  }
  return invoke("open_url", { url });
}
