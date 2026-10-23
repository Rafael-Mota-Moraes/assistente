// Comandos que só existem no app desktop (app/src-tauri/src/main.rs). No navegador
// (pnpm dev sem Tauri) caem em valores neutros para a UI continuar navegável.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import type { ExecResult, HubConnection, PickedElement, VmStatus } from "./types";

const DEV_HUB_URL = "http://127.0.0.1:7878";

/** Evento do modo apontar (F12.2): o Rust intercepta a navegação `assistente-pick:`. */
export const PREVIEW_PICK_EVENT = "preview-pick";

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

/** Roda no PC o pedido `host` que o hubd guardou como aprovado (F9.10). O app desktop é o
 * único que roda: no navegador (pnpm dev) não há como executar no host. */
export function hostExec(id: string): Promise<ExecResult> {
  if (!isTauri()) return Promise.reject(new Error("só funciona no app desktop"));
  return invoke<ExecResult>("host_exec", { id });
}

/** Pedido `host` pendente cujo comando está, texto exato, na lista do PC do projeto
 * (F9.11): o app confere a lista no host e roda sem clique. `null` = fora da lista (ou fora
 * do app desktop): o card espera você. */
export function hostAutoExec(id: string): Promise<ExecResult | null> {
  if (!isTauri()) return Promise.resolve(null);
  return invoke<ExecResult | null>("host_auto_exec", { id });
}

/** Lista do PC do projeto (F9.11), guardada no host. `null` fora do app desktop. */
export function pcAllowedCommands(profile: string, project: string): Promise<string[] | null> {
  if (!isTauri()) return Promise.resolve(null);
  return invoke<string[]>("pc_allowed_commands", { profile, project }).catch(() => null);
}

export function setPcAllowedCommands(
  profile: string,
  project: string,
  commands: string[],
): Promise<void> {
  if (!isTauri()) return Promise.reject(new Error("só funciona no app desktop"));
  return invoke("set_pc_allowed_commands", { profile, project, commands });
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

/** Preview do projeto (F12.1): webview filha ancorada sobre a área de conteúdo, fora das
 * capabilities (a página apontada não alcança os comandos do app). No navegador (pnpm dev)
 * o painel usa iframe e aqui não faz nada. Chamar de novo recarrega a página. */
export function openPreviewPanel(url: string, rect: PanelRect): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke("open_preview_panel", { url, rect });
}

/** Acompanha o âncora do preview (janela redimensionou, layout mudou). */
export function updatePreviewPanelBounds(rect: PanelRect): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke("update_preview_panel_bounds", { rect });
}

/** Esconde ou mostra a webview do preview: um modal do app precisa ficar por cima dela. */
export function previewPanelVisible(visible: boolean): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke("preview_panel_visible", { visible });
}

/** Liga/desliga o modo "apontar" na página do preview (F12.1/F12.2). */
export function previewPicker(enabled: boolean): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke("preview_picker", { enabled });
}

/** Fecha a webview do preview. */
export function closePreviewPanel(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke("close_preview_panel");
}

/** Abre uma URL no navegador padrão (links clicáveis do terminal). */
export function openUrl(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener");
    return Promise.resolve();
  }
  return invoke("open_url", { url });
}

/** Assina o pick do modo apontar (F12.2). No navegador (pnpm dev) não acontece nada:
 *  o pick vem da webview do preview, que só existe no app desktop. */
export function listenPreviewPick(handler: (pick: PickedElement) => void): Promise<UnlistenFn> {
  if (!isTauri()) return Promise.resolve(() => {});
  return listen<PickedElement>(PREVIEW_PICK_EVENT, (event) => handler(event.payload));
}
