import type {
  CollectReport,
  Health,
  HubConnection,
  MemoryStatus,
  NotebookStatus,
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
} from "./types";

export class HubError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

function settingsPath(profile: string, project: string): string {
  return `/api/projects/${encodeURIComponent(profile)}/${encodeURIComponent(project)}/settings`;
}

/** Cliente HTTP do hubd. O WebSocket dos terminais fica em terminal.ts. */
export class HubClient {
  constructor(
    private readonly connection: HubConnection,
    private readonly fetchFn: typeof fetch = (...args) => fetch(...args),
  ) {}

  health(): Promise<Health> {
    return this.get("/api/health");
  }

  profiles(): Promise<ProfileView[]> {
    return this.get("/api/profiles");
  }

  projects(profile: string): Promise<Project[]> {
    return this.get(`/api/projects?profile=${encodeURIComponent(profile)}`);
  }

  /** Configurações do projeto que afrouxam o jail (F9.2). */
  projectSettings(profile: string, project: string): Promise<ProjectSettingsView> {
    return this.get(settingsPath(profile, project));
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
