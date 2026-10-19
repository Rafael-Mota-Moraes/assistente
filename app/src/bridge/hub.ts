import type { Health, HubConnection, NotebookStatus, ProfileView, Project } from "./types";

export class HubError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** Cliente HTTP do hubd. O WebSocket dos terminais entra na F2.3. */
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

  notebook(): Promise<NotebookStatus> {
    return this.get("/api/notebook");
  }

  private async get<T>(path: string): Promise<T> {
    const headers: Record<string, string> = {};
    if (this.connection.token) headers.Authorization = `Bearer ${this.connection.token}`;
    let response: Response;
    try {
      response = await this.fetchFn(`${this.connection.url}${path}`, { headers });
    } catch {
      throw new HubError(0, `hubd fora do ar em ${this.connection.url}`);
    }
    if (!response.ok) {
      const body = (await response.json().catch(() => ({}))) as { error?: string };
      throw new HubError(response.status, body.error ?? `HTTP ${response.status}`);
    }
    return (await response.json()) as T;
  }
}
