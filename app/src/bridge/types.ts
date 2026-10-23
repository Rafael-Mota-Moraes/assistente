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
  /** F14.4: o processo do painel já saiu; a sessão existe mas está morta. A UI usa para marcar a aba. */
  dead: boolean;
};

export type SessionSpec = { profile: string; project: string; agent: AgentKind };

/** Afrouxa o ai-jail do projeto (F9.2); guardado na VM em ~/.assistente/projetos.toml. */
export type ProjectSettings = {
  docker: boolean;
  allow_hosts: string[];
  full_network: boolean;
  allowed_commands: string[];
  /** F12.1: endereço do dev server, visto do PC. Ausente = sem preview. */
  preview_url?: string | null;
};

/** Pedido de um agente para rodar um comando fora do jail (F9.5). */
export type ExecState = "pending" | "running" | "done" | "denied" | "expired" | "failed";

/** Onde o comando de um pedido roda (F9.9): `vm` = fora do jail na VM; `host` = no PC. */
export type ExecTarget = "vm" | "host";

export type ExecResult = {
  exit_code: number | null;
  stdout: string;
  stderr: string;
  truncated: boolean;
  timed_out: boolean;
  duration_ms: number;
};

/** O que o app devolve ao hubd depois de rodar no PC (F9.10): o resultado ou o motivo da
 * falha (o comando nem começou). */
export type HostExecOutcome = ExecResult | { error: string };

/** GET /api/exec: pendentes primeiro, depois os recentes (30 min). */
export type ExecView = {
  id: string;
  /** Sessão (aba) que pediu. */
  session: string;
  profile: string;
  project: string;
  cwd: string;
  command: string;
  /** `host` = pedido para o PC (F9.9): o hubd não executa, o app sim (F9.10). */
  target: ExecTarget;
  timeout_secs: number;
  created_at: number;
  state: ExecState;
  auto_approved: boolean;
  result: ExecResult | null;
  /** Pedido `host` já foi pego pelo app para execução no PC (F9.11). */
  picked_up?: boolean;
};

/** GET/PUT /api/projects/{profile}/{project}/settings. */
export type ProjectSettingsView = {
  settings: ProjectSettings;
  /** `[jail] docker = true` no .assistente.toml do repo liga o Docker de qualquer jeito. */
  docker_from_project_file: boolean;
};

/** Quadro do projeto (F11): `.assistente/kanban.json` no repo, espelho de hubd `kanban`. */
/** `validado` = passou no QA e foi juntado numa branch (F11.15). */
export type KanbanColumn = "backlog" | "fazendo" | "feito" | "validado";
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
  /** Parou sem culpa da tarefa (cota esgotada, F11.14): a linha de erro do provedor. */
  blocked?: string;
  /** Onde o QA juntou a tarefa (`validado`, F11.15). */
  merged?: KanbanMerged;
  /** IDs das tarefas das quais esta depende (F11.13). */
  depends_on?: string[];
  created_at: number;
};

/** Branch que recebeu a tarefa e o commit dela depois da junção. */
export type KanbanMerged = { branch: string; commit: string };

/** Orquestrador automático do projeto (F11.11); ausente = desligado. */
export type KanbanOrchestrator = {
  enabled: boolean;
  max_workers: number;
  /** Somadas às regras padrão no prompt de todo trabalhador do projeto. No `PUT`, ausente mantém. */
  instructions?: string;
  /** Pausado por cota esgotada (F11.14): nada abre sozinho até "Retomar". O `PUT` não mexe. */
  paused?: KanbanPause;
};

/** Por que e quando (segundos Unix) o orquestrador pausou. */
export type KanbanPause = { reason: string; at: number };

export type KanbanBoard = {
  next: number;
  cards: KanbanCard[];
  orchestrator?: KanbanOrchestrator;
  /** Último erro do orquestrador automático (só em memória no hubd). */
  orchestrator_error?: string;
  /** Só na resposta de `setOrchestrator`: tarefas que a volta abriu agora. */
  started?: string[];
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
  /** IDs das tarefas das quais esta depende (F11.13). Na edição, ausente = não muda; vazio = limpa. */
  depends_on?: string[];
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

/** Contexto de código do elemento apontado (F12.3). */
export type PickCodeContext = {
  /** A página usa React? */
  react: boolean;
  /** Componente mais próximo do elemento (null se a página não usa React). */
  component: string | null;
  /** Pilha de componentes, do mais interno ao mais externo. */
  stack: string[];
  /** Arquivo e linha do JSX (null em build de produção). */
  source: { file: string; line: number | null; column: number | null } | null;
};

/** Elemento apontado na página do preview (F12.2). Espelha `app/src-tauri/src/pick.rs`. */
export type PickedElement = {
  /** Tag do elemento (`button`, `div`…). */
  tag: string;
  /** `id` do elemento, se tiver. */
  id: string | null;
  /** Classes do elemento. */
  classes: string[];
  /** `role` do elemento, se tiver. */
  role: string | null;
  /** Atributos `aria-*`, sem o prefixo (`label`, `describedby`…). */
  aria: Record<string, string>;
  /** Texto curto do elemento. */
  text: string | null;
  /** Trecho de HTML do elemento. */
  html: string | null;
  /** Seletor CSS estável. */
  selector: string;
  /** Caminho do elemento no DOM. */
  dom_path: string;
  /** Retângulo do elemento na viewport (px de CSS). */
  rect: { x: number; y: number; width: number; height: number };
  /** Tamanho da janela da página. */
  viewport: { width: number; height: number };
  /** Rolagem da página. */
  scroll: { x: number; y: number };
  /** URL real da webview (o Rust troca pela URL carregada; a do payload não vale). */
  url: string;
  /** Contexto de código (F12.3). */
  code?: PickCodeContext | null;
};

/** Elemento apontado na página (F12.2) e contexto de código (F12.3). */
export type PickDetail = {
  /** URL real da webview (o hubd não confia na URL do payload). */
  url: string;
  /** Seletor CSS estável do elemento. */
  selector: string;
  /** Texto curto do elemento. */
  text: string;
  /** Trecho de HTML. */
  html: string;
  /** Tag do elemento. */
  tag: string;
  id?: string | null;
  classes?: string[];
  role?: string | null;
  aria_label?: string | null;
  /** Caminho no DOM. */
  dom_path?: string | null;
  /** Componente React (F12.3). */
  component?: string | null;
  /** `arquivo:linha` do fonte do componente (F12.3). */
  source?: string | null;
  /** Pilha de componentes (F12.3). */
  components?: string[];
  /** Do F12.2, sem campo próprio no hubd: vão no `extra` e ficam no arquivo do pick. */
  aria?: Record<string, string>;
  rect?: { x: number; y: number; width: number; height: number };
  viewport?: { width: number; height: number };
  scroll?: { x: number; y: number };
};

/** Onde o pedido vai (F12.4): na sessão de um agente ou no quadro. */
export type PickTarget = "sessao" | "quadro";

/** O que o app manda para o hubd (F12.4). */
export type PickRequest = {
  target: PickTarget;
  /** Sessão que recebe o pedido (alvo = sessão). */
  session?: string | null;
  /** O recado do usuário. */
  request: string;
  pick: PickDetail;
};

/** O que o hubd devolve (F12.4). */
export type PickResult = {
  target: PickTarget;
  /** A sessão que recebeu o pedido (alvo = sessão). */
  session?: string;
  /** O card criado no quadro (alvo = quadro). */
  card?: KanbanCard;
  /** O prompt que o agente recebeu. */
  prompt: string;
  /** O arquivo do pick, dentro do projeto. */
  file: string;
};

/** O prompt que será enviado (F12.4). */
export type PickPreview = { prompt: string };
