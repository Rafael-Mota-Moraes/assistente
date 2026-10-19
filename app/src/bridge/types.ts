// Formatos da API do hubd (crates/hubd/src/api.rs). Mantenha os dois lados iguais.

export type Health = { ok: boolean; version: string; uptime_seconds: number };

export type ProfileView = { id: string; label: string; color: string; root: string };

export type Project = { name: string; path: string; has_project_config: boolean };

export type ServiceStatus = { name: string; port: number; up: boolean };

export type NotebookStatus = {
  ssh_host: string;
  tunnel_managed: boolean;
  services: ServiceStatus[];
};

export type HubConnection = { url: string; token: string | null };

export type VmState = "running" | "stopped" | "missing" | "no-incus" | "unknown";

export type VmStatus = { state: VmState; detail: string | null };
