// Comandos que só existem no app desktop (app/src-tauri/src/main.rs). No navegador
// (pnpm dev sem Tauri) caem em valores neutros para a UI continuar navegável.
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { HubConnection, VmStatus } from "./types";

const DEV_HUB_URL = "http://127.0.0.1:7878";

export function hubConnection(): Promise<HubConnection> {
  if (!isTauri()) return Promise.resolve({ url: DEV_HUB_URL, token: null });
  return invoke<HubConnection>("hub_connection");
}

export function vmStatus(): Promise<VmStatus> {
  if (!isTauri()) return Promise.resolve({ state: "unknown", detail: "fora do app desktop" });
  return invoke<VmStatus>("vm_status");
}

export function openInCursor(path: string): Promise<void> {
  if (!isTauri()) return Promise.reject(new Error("só funciona no app desktop"));
  return invoke("open_in_cursor", { path });
}
