import type {
  CollectReport,
  ExecView,
  FileContent,
  FileList,
  GitDiff,
  GitStatus,
  Health,
  HostExecOutcome,
  HubConnection,
  KanbanBoard,
  KanbanCard,
  KanbanDraft,
  KanbanOrchestrator,
  MainApplyResult,
  MemoryStatus,
  NotebookStatus,
  PickDetail,
  PickPreview,
  PickRequest,
  PickResult,
  ProfileView,
  Project,
  ProjectSettings,
  ProjectSettingsView,
  SessionInfo,
  SessionSpec,
  SleepRequest,
  SleepStartRequest,
  SleepStatus,
  StartedSleep,
  UpdateStatus,
} from "./types";

export class HubError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

function kanbanPath(profile: string, project: string): string {
  return `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/kanban`;
}

function settingsPath(profile: string, project: string): string {
  return `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/settings`;
}

function picksPath(profile: string, project: string): string {
  return `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/picks`;
}

/** Cliente HTTP do hubd. O WebSocket dos terminais fica em terminal.ts. */
export class HubClient {
  constructor(
    private readonly connection: HubConnection,
    private readonly fetchFn: typeof fetch = (...args) => fetch(...args),
  ) {}

  /** Entrada da web UI do ai-memory (F5.5): o hubd troca o token pelo cookie da janela. */
  memoryLoginUrl(): string {
    const base = this.connection.url.replace(/\/$/, "");
    const query = this.connection.token
      ? `?token=${encodeURIComponent(this.connection.token)}`
      : "";
    return `${base}/memoria/entrar${query}`;
  }

  health(): Promise<Health> {
    return this.get("/api/health");
  }

  profiles(): Promise<ProfileView[]> {
    return this.get("/api/profiles");
  }

  projects(profile: string): Promise<Project[]> {
    return this.get(`/api/projects?profile=${encodeURIComponent(profile)}`);
  }

  /** Quadro do projeto (F11.1); soltar tarefas presas a sessões mortas acontece aqui. */
  kanban(profile: string, project: string): Promise<KanbanBoard> {
    return this.get(kanbanPath(profile, project));
  }

  createCard(profile: string, project: string, draft: KanbanDraft): Promise<KanbanCard> {
    return this.request("POST", kanbanPath(profile, project), draft);
  }

  /** Edita um card ou move de coluna (`column`). */
  updateCard(
    profile: string,
    project: string,
    id: string,
    draft: KanbanDraft,
  ): Promise<KanbanCard> {
    return this.request(
      "PATCH",
      `${kanbanPath(profile, project)}/${encodeURIComponent(id)}`,
      draft,
    );
  }

  /** Liga ou desliga o orquestrador automático (F11.11); devolve o quadro depois da 1ª volta. */
  setOrchestrator(
    profile: string,
    project: string,
    orchestrator: KanbanOrchestrator,
  ): Promise<KanbanBoard> {
    return this.request("PUT", `${kanbanPath(profile, project)}/orquestrador`, orchestrator);
  }

  async deleteCard(profile: string, project: string, id: string): Promise<void> {
    await this.request("DELETE", `${kanbanPath(profile, project)}/${encodeURIComponent(id)}`);
  }

  /** Manda uma tarefa `manual` para um trabalhador (F11.6). `models` vazio = os do projeto. */
  mandarCard(
    profile: string,
    project: string,
    id: string,
    models: string[] = [],
  ): Promise<SessionInfo> {
    return this.request(
      "POST",
      `${kanbanPath(profile, project)}/${encodeURIComponent(id)}/mandar`,
      { models },
    );
  }

  /** Abre um Claude que monta o quadro a partir do backlog em markdown (F11.9). */
  explorarBacklog(profile: string, project: string): Promise<SessionInfo> {
    return this.request("POST", `${kanbanPath(profile, project)}/explorar`);
  }

  /** Relança um trabalhador com o mesmo modelo, no mesmo worktree (F11.6). */
  restartWorker(session: string): Promise<SessionInfo> {
    return this.request("POST", `/api/sessions/${encodeURIComponent(session)}/reiniciar`);
  }

  /** Configurações do projeto que afrouxam o jail (F9.2). */
  projectSettings(profile: string, project: string): Promise<ProjectSettingsView> {
    return this.get(settingsPath(profile, project));
  }

  /** Envia o pedido apontado na tela (F12.4): cola na sessão do agente ou vira card no quadro. */
  sendPick(profile: string, project: string, body: PickRequest): Promise<PickResult> {
    return this.request("POST", picksPath(profile, project), body);
  }

  /** O prompt que o agente vai receber (F12.4): o modal mostra antes de enviar. */
  previewPick(
    profile: string,
    project: string,
    body: { pick: PickDetail; request: string },
  ): Promise<PickPreview> {
    return this.request("POST", `${picksPath(profile, project)}/preview`, body);
  }

  /** F10.1: árvore de arquivos do projeto. */
  listFiles(profile: string, project: string): Promise<FileList> {
    return this.get(
      `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/files`,
    );
  }

  /** F10.1: lê um arquivo (texto UTF-8). */
  readFile(profile: string, project: string, path: string): Promise<FileContent> {
    return this.get(
      `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/files/${encodeURIComponent(path)}`,
    );
  }

  /** F10.1: grava um arquivo (texto UTF-8). */
  async writeFile(profile: string, project: string, path: string, content: string): Promise<void> {
    await this.request(
      "PUT",
      `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/files/${encodeURIComponent(path)}`,
      { content },
    );
  }

  /** F10.1: status git dos arquivos alterados. */
  gitStatus(profile: string, project: string): Promise<GitStatus> {
    return this.get(
      `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/git/status`,
    );
  }

  /** F10.1/F10.4: conteúdo de um arquivo em dois commits (ou HEAD × árvore de trabalho). */
  gitDiff(
    profile: string,
    project: string,
    path: string,
    from?: string,
    to?: string,
  ): Promise<GitDiff> {
    const q = new URLSearchParams();
    if (from) q.set("from", from);
    if (to) q.set("to", to);
    const qs = q.toString() ? `?${q.toString()}` : "";
    return this.get(
      `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/git/diff/${encodeURIComponent(path)}${qs}`,
    );
  }

  saveProjectSettings(
    profile: string,
    project: string,
    settings: ProjectSettings,
  ): Promise<ProjectSettingsView> {
    return this.request("PUT", settingsPath(profile, project), settings);
  }

  notebook(): Promise<NotebookStatus> {
    return this.get("/api/notebook");
  }

  memory(): Promise<MemoryStatus> {
    return this.get("/api/memory");
  }

  sessions(): Promise<SessionInfo[]> {
    return this.get("/api/sessions");
  }

  createSession(spec: SessionSpec): Promise<SessionInfo> {
    return this.request("POST", "/api/sessions", spec);
  }

  async deleteSession(id: string): Promise<void> {
    await this.request("DELETE", `/api/sessions/${encodeURIComponent(id)}`);
  }

  /** Hora de dormir (F6.3): entrega o projeto ao notebook; 409 se a árvore está suja. */
  startSleep(request: SleepStartRequest): Promise<StartedSleep> {
    return this.request("POST", "/api/sleep", request);
  }

  /** O dorme está rodando? + tail do log no notebook (F6.3). */
  sleepStatus(profile: string, project: string): Promise<SleepStatus> {
    return this.get(
      `/api/sleep?profile=${encodeURIComponent(profile)}&project=${encodeURIComponent(project)}`,
    );
  }

  /** Pede parada do modo dormir (cria o STOP no notebook; a iteração atual termina). */
  async stopSleep(): Promise<void> {
    await this.request("POST", "/api/sleep/stop");
  }

  /** Manhã (F6.5): busca dorme/<data> e agente/* do notebook e resume o que chegou. */
  collectSleep(request: SleepRequest): Promise<CollectReport> {
    return this.request("POST", "/api/sleep/collect", request);
  }

  /** Merge fast-forward da branch recolhida; 409 se a base andou durante a noite. */
  async mergeSleep(request: SleepRequest): Promise<void> {
    await this.request("POST", "/api/sleep/merge", request);
  }

  /** Pedidos dos agentes para rodar fora do jail (F9.5). */
  execRequests(): Promise<ExecView[]> {
    return this.get("/api/exec");
  }

  /** Um pedido só (F9.10): o app confere com o hubd o que foi aprovado antes de rodar no PC. */
  execRequest(id: string): Promise<ExecView> {
    return this.get(`/api/exec/${encodeURIComponent(id)}`);
  }

  /** Roda o pedido na VM; `always` grava o comando nos sempre permitidos do projeto. */
  async approveExec(id: string, always = false): Promise<void> {
    await this.request("POST", `/api/exec/${encodeURIComponent(id)}/approve`, { always });
  }

  async denyExec(id: string): Promise<void> {
    await this.request("POST", `/api/exec/${encodeURIComponent(id)}/deny`);
  }

  /** Devolve ao hubd o que rodou no PC (F9.10): o agente recebe pelo long-poll. */
  async execResult(id: string, outcome: HostExecOutcome): Promise<void> {
    await this.request("POST", `/api/exec/${encodeURIComponent(id)}/result`, outcome);
  }

  /** Pega o pedido `host` para execução no PC (F9.11): evita execução dupla. */
  async claimExec(id: string): Promise<{
    id: string;
    command: string;
    cwd: string;
    env: [string, string][];
    timeout_secs: number;
  }> {
    return this.request("POST", `/api/exec/${encodeURIComponent(id)}/claim`);
  }

  /** Status do atualizador (F8.5/F8.6). */
  updateStatus(): Promise<UpdateStatus> {
    return this.get("/api/update");
  }

  /** Aplica um commit (dispara o atualizador). */
  applyUpdate(commit: string): Promise<{ commit: string; started: boolean }> {
    return this.request("POST", "/api/update/apply", { commit });
  }

  /** Volta para a versão anterior. */
  async rollbackUpdate(): Promise<void> {
    await this.request("POST", "/api/update/rollback");
  }

  /** Leva commits escolhidos para a `main` do projeto do Assistente (F8.8). */
  applyToMain(commits: string[]): Promise<MainApplyResult> {
    return this.request("POST", "/api/update/main", { commits });
  }

  /** URL do WebSocket do terminal; o token vai na query (WebSocket não manda header). */
  terminalUrl(id: string): string {
    const base = this.connection.url.replace(/^http/, "ws");
    const query = this.connection.token
      ? `?token=${encodeURIComponent(this.connection.token)}`
      : "";
    return `${base}/api/sessions/${encodeURIComponent(id)}/terminal${query}`;
  }

  private get<T>(path: string): Promise<T> {
    return this.request("GET", path);
  }

  private async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = {};
    if (this.connection.token) headers.Authorization = `Bearer ${this.connection.token}`;
    const init: RequestInit = { headers };
    if (method !== "GET") init.method = method;
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(body);
    }
    let response: Response;
    try {
      response = await this.fetchFn(`${this.connection.url}${path}`, init);
    } catch {
      throw new HubError(0, `hubd fora do ar em ${this.connection.url}`);
    }
    if (!response.ok) {
      const body = (await response.json().catch(() => ({}))) as { error?: string };
      throw new HubError(response.status, body.error ?? `HTTP ${response.status}`);
    }
    if (response.status === 204) return undefined as T;
    return (await response.json()) as T;
  }
}
