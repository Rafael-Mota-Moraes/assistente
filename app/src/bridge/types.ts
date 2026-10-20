// Formatos da API do hubd (crates/hubd/src/api.rs). Mantenha os dois lados iguais.

export type Health = {
  ok: boolean;
  version: string;
  uptime_seconds: number;
  /** Commit do binário do hubd e da UI servida (`null` quando o hubd não serve UI). */
  commit: { hubd: string; ui: string | null };
};

export type ProfileView = { id: string; label: string; color: string; root: string };

export type Project = { name: string; path: string; has_project_config: boolean };

export type ServiceStatus = {
  name: string;
  port: number;
  up: boolean;
  /** Por que o serviço está caído; null quando de pé. */
  last_error: string | null;
  /** Segundos Unix da última verificação. */
  checked_at: number;
};

export type TunnelStatus = {
  state: "connecting" | "up" | "down";
  last_error: string | null;
  restarts: number;
  since: number;
};

export type NotebookStatus = {
  ssh_host: string;
  tunnel_managed: boolean;
  tunnel: TunnelStatus | null;
  services: ServiceStatus[];
};

/** Servidor ai-memory local na VM (F5.0). */
export type MemoryStatus = {
  port: number;
  mirror_dir: string;
  up: boolean;
  last_error: string | null;
  checked_at: number;
};

export type HubConnection = { url: string; token: string | null };

export type VmState = "running" | "stopped" | "missing" | "no-incus" | "unknown";

export type VmStatus = { state: VmState; detail: string | null };

export type AgentKind = "claude" | "opencode";

export type SessionInfo = {
  id: string;
  profile: string;
  project: string;
  agent: AgentKind;
  created_at: number;
  attached: number;
};

export type SessionSpec = { profile: string; project: string; agent: AgentKind };

export type CollectedCommit = { hash: string; subject: string };

/** Resumo da manhã (POST /api/sleep/collect, F6.5). */
export type CollectReport = {
  base: string;
  sleep_branch: string;
  commits: CollectedCommit[];
  relatorio: string | null;
  fast_forward: boolean;
};

export type SleepRequest = { profile: string; project: string; date: string };

export type SleepStartRequest = { profile: string; project: string; wip?: boolean };

export type StartedSleep = {
  date: string;
  session: string;
  remote_dir: string;
  prompt_file: string;
  log_file: string;
};

export type SleepStatus = { sleeping: boolean; log_tail: string | null };
