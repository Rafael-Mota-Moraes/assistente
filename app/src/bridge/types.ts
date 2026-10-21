// Formatos da API do hubd (crates/hubd/src/api.rs). Mantenha os dois lados iguais.

export type Health = {
  ok: boolean;
  version: string;
  uptime_seconds: number;
  /** Commit do binário do hubd e da UI servida (`null` quando o hubd não serve UI). */
  commit: { hubd: string; ui: string | null };
  /** Aba/painel Assistente (F8.6): projeto do próprio Assistente a ser editado pelo agente. */
  self_update: { profile: string; project: string } | null;
};

export type UpdateCommit = { hash: string; subject: string };

export type UpdateStatus = {
  running: boolean;
  status: string;
  commit: string | null;
  current_ui: string | null;
  previous_commit: string | null;
  /** F8.7: o commit em uso mexeu em `app/src-tauri`; a casca do app precisa ser reinstalada. */
  shell_changed: boolean;
  error: string | null;
  log_tail: string | null;
  log: string | null;
  /** HEAD do projeto do próprio Assistente. */
  head: string | null;
  head_commit: string | null;
  /** Commits recentes (mais novo primeiro), para escolher o que aplicar. */
  commits: UpdateCommit[];
  pending_commits: UpdateCommit[];
};

/** Resultado de juntar commits na `main` (F8.8, `POST /api/update/main`). */
export type MainApplyResult = {
  /** HEAD curto da `main` depois de aplicar. */
  main: string;
  /** Commits aplicados, do mais antigo para o mais novo. */
  applied: UpdateCommit[];
};

export type ProfileView = {
  id: string;
  label: string;
  color: string;
  root: string;
  /** `false` = o Claude deste perfil nunca fez login (`assistente login claude <perfil>`). */
  claude_login: boolean;
};

export type Project = {
  name: string;
  path: string;
  has_project_config: boolean;
  /** Tarefas do quadro ainda não feitas (backlog + fazendo), F11.3. */
  kanban_pendentes: number;
};

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

/** Quadro do projeto (F11): `.assistente/kanban.json` no repo, espelho de hubd `kanban`. */
export type KanbanColumn = "backlog" | "fazendo" | "feito";
/** `manual` só roda quando você manda; `agente` é puxado pelo orquestrador (F11.2). */
export type KanbanKind = "manual" | "agente";
export type KanbanPriority = "alta" | "normal" | "baixa";

export type KanbanCard = {
  id: string;
  title: string;
  description: string;
  priority: KanbanPriority;
  kind: KanbanKind;
  column: KanbanColumn;
  /** Sessão que segura a tarefa ("fazendo"). */
  holder?: string;
  commit?: string;
  /** Vezes que voltou do "fazendo" sem concluir; o orquestrador automático pula (F11.11). */
  failures?: number;
  created_at: number;
};

/** Orquestrador automático do projeto (F11.11); ausente = desligado. */
export type KanbanOrchestrator = { enabled: boolean; max_workers: number };

export type KanbanBoard = {
  next: number;
  cards: KanbanCard[];
  orchestrator?: KanbanOrchestrator;
  /** Último erro do orquestrador automático (só em memória no hubd). */
  orchestrator_error?: string;
};

/** Campos de criar/editar; todos opcionais na edição. */
export type KanbanDraft = {
  title?: string;
  description?: string;
  priority?: KanbanPriority;
  kind?: KanbanKind;
  column?: KanbanColumn;
  /** `0` = "Tentar de novo": o orquestrador automático volta a puxar. */
  failures?: number;
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
