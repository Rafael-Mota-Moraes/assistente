// Formatos da API do hubd (crates/hubd/src/api.rs). Mantenha os dois lados iguais.

export type Health = {
  ok: boolean;
  version: string;
  uptime_seconds: number;
  /** Commit do binário do hubd e da UI servida (`null` quando o hubd não serve UI). */
  commit: { hubd: string; ui: string | null };
};

export type ProfileView = {
  id: string;
  label: string;
  color: string;
  root: string;
  /** `false` = o Claude deste perfil nunca fez login (`assistente login claude <perfil>`). */
  claude_login: boolean;
};

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
  data_dir: string;
  mirror_dir: string;
  up: boolean;
  last_error: string | null;
  checked_at: number;
  server_version: string | null;
  last_commit: string | null;
  web_url: string;
};

export type HubConnection = { url: string; token: string | null };

export type VmState = "running" | "stopped" | "missing" | "no-incus" | "unknown";

export type VmStatus = { state: VmState; detail: string | null };

/** `shell` = aba Terminal: bash na VM, fora do jail (F9.1). */
export type AgentKind = "claude" | "opencode" | "shell";

export type SessionInfo = {
  id: string;
  profile: string;
  project: string;
  agent: AgentKind;
  created_at: number;
  attached: number;
};

export type SessionSpec = { profile: string; project: string; agent: AgentKind };

/** Afrouxa o ai-jail do projeto (F9.2); guardado na VM em ~/.assistente/projetos.toml. */
export type ProjectSettings = {
  docker: boolean;
  allow_hosts: string[];
  full_network: boolean;
  allowed_commands: string[];
};

/** Pedido de um agente para rodar um comando fora do jail (F9.5). */
export type ExecState = "pending" | "running" | "done" | "denied" | "expired" | "failed";

export type ExecResult = {
  exit_code: number | null;
  stdout: string;
  stderr: string;
  truncated: boolean;
  timed_out: boolean;
  duration_ms: number;
};

/** GET /api/exec: pendentes primeiro, depois os recentes (30 min). */
export type ExecView = {
  id: string;
  /** Sessão (aba) que pediu. */
  session: string;
  profile: string;
  project: string;
  cwd: string;
  command: string;
  timeout_secs: number;
  created_at: number;
  state: ExecState;
  auto_approved: boolean;
  result: ExecResult | null;
};

/** GET/PUT /api/projects/{profile}/{project}/settings. */
export type ProjectSettingsView = {
  settings: ProjectSettings;
  /** `[jail] docker = true` no .assistente.toml do repo liga o Docker de qualquer jeito. */
  docker_from_project_file: boolean;
};

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

/** F10.1: árvore de arquivos do projeto (sem .git nem ignorados). */
export type FileList = { files: string[] };

/** F10.1: conteúdo de um arquivo (texto UTF-8). */
export type FileContent = { path: string; content: string };

/** F10.1: status git de um arquivo. */
export type GitFileStatus = {
  path: string;
  status: "modified" | "added" | "deleted" | "renamed" | "copied" | "untracked";
  staged: boolean;
};

export type GitStatus = { files: GitFileStatus[] };

/** F10.1: diff de um arquivo (HEAD × árvore de trabalho). */
export type GitDiff = { path: string; head: string | null; working: string | null };
