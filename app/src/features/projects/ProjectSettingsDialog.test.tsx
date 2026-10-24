import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { pcAllowedCommands, setPcAllowedCommands } from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import type { ProjectSettingsView } from "../../bridge/types";
import { useAppStore } from "../../store";
import { ProjectSettingsDialog } from "./ProjectSettingsDialog";

// Lista do PC (F9.11): fora do app desktop não aparece.
vi.mock("../../bridge/host", () => ({
  pcAllowedCommands: vi.fn().mockResolvedValue(null),
  setPcAllowedCommands: vi.fn().mockResolvedValue(undefined),
}));

const closed = (): ProjectSettingsView => ({
  settings: {
    docker: false,
    allow_hosts: [],
    full_network: false,
    allowed_commands: [],
    preview_url: null,
    check: "./dev just check",
  },
  docker_from_project_file: false,
});

function open(view: ProjectSettingsView) {
  const hub = {
    projectSettings: vi.fn().mockResolvedValue(view),
    saveProjectSettings: vi.fn().mockImplementation(async (_p, _proj, settings) => ({
      settings,
      docker_from_project_file: view.docker_from_project_file,
    })),
  };
  useAppStore.setState({
    hub: hub as unknown as HubClient,
    projectSettings: { profile: "pessoal", project: "game" },
  });
  render(<ProjectSettingsDialog />);
  return hub;
}

describe("ProjectSettingsDialog", () => {
  it("afrouxa o jail e salva para as próximas sessões", async () => {
    const hub = open(closed());
    expect(hub.projectSettings).toHaveBeenCalledWith("pessoal", "game");

    fireEvent.click(await screen.findByRole("checkbox", { name: /Docker/ }));
    fireEvent.change(screen.getByLabelText("Novo em Hosts liberados"), {
      target: { value: " pypi.org " },
    });
    fireEvent.click(screen.getByTitle("Adicionar em Hosts liberados"));
    fireEvent.change(screen.getByLabelText("Novo em Comandos sempre permitidos"), {
      target: { value: "cargo build" },
    });
    fireEvent.click(screen.getByTitle("Adicionar em Comandos sempre permitidos"));
    expect(screen.getByText(/pode editá-los/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await vi.waitFor(() => expect(useAppStore.getState().projectSettings).toBeNull());
    expect(hub.saveProjectSettings).toHaveBeenCalledWith("pessoal", "game", {
      docker: true,
      allow_hosts: ["pypi.org"],
      full_network: false,
      allowed_commands: ["cargo build"],
      preview_url: null,
      check: "./dev just check",
    });
    expect(useAppStore.getState().toast).toMatch(/próximas sessões/);
  });

  it("guarda a URL do preview e avisa que o endereço é visto do PC", async () => {
    const hub = open(closed());
    const campo = await screen.findByLabelText("URL do preview");
    expect(screen.getByText(/visto do PC/)).toBeInTheDocument();

    fireEvent.change(campo, { target: { value: " http://10.77.0.10:5173 " } });
    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await vi.waitFor(() => expect(useAppStore.getState().projectSettings).toBeNull());
    expect(hub.saveProjectSettings).toHaveBeenCalledWith(
      "pessoal",
      "game",
      expect.objectContaining({ preview_url: "http://10.77.0.10:5173" }),
    );
  });

  it("campo vazio do preview salva sem preview", async () => {
    const hub = open({
      ...closed(),
      settings: { ...closed().settings, preview_url: "http://10.77.0.10:5173" },
    });
    const campo = await screen.findByLabelText("URL do preview");
    expect(campo).toHaveValue("http://10.77.0.10:5173");

    fireEvent.change(campo, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await vi.waitFor(() => expect(useAppStore.getState().projectSettings).toBeNull());
    expect(hub.saveProjectSettings).toHaveBeenCalledWith(
      "pessoal",
      "game",
      expect.objectContaining({ preview_url: null }),
    );
  });

  it("guarda o check do QA", async () => {
    const hub = open(closed());
    const field = await screen.findByLabelText("Check do QA");
    expect(field).toHaveValue("./dev just check");
    fireEvent.change(field, { target: { value: "cargo test" } });
    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await vi.waitFor(() => expect(useAppStore.getState().projectSettings).toBeNull());
    expect(hub.saveProjectSettings).toHaveBeenCalledWith(
      "pessoal",
      "game",
      expect.objectContaining({ check: "cargo test" }),
    );
  });

  it("check do QA vazio volta ao padrão", async () => {
    const hub = open(closed());
    fireEvent.change(await screen.findByLabelText("Check do QA"), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await vi.waitFor(() => expect(useAppStore.getState().projectSettings).toBeNull());
    expect(hub.saveProjectSettings).toHaveBeenCalledWith(
      "pessoal",
      "game",
      expect.objectContaining({ check: "./dev just check" }),
    );
  });

  it("rede completa mostra o aviso e desliga a lista de hosts", async () => {
    open(closed());
    fireEvent.click(await screen.findByRole("checkbox", { name: /Rede completa/ }));
    expect(screen.getByText(/rede local/)).toBeInTheDocument();
    expect(screen.getByLabelText("Novo em Hosts liberados")).toBeDisabled();
  });

  it("mostra quando o Docker vem do .assistente.toml do repo", async () => {
    open({ ...closed(), docker_from_project_file: true });
    const docker = await screen.findByRole("checkbox", { name: /Docker/ });
    expect(docker).toBeChecked();
    expect(docker).toBeDisabled();
    expect(screen.getByText(/do .assistente.toml do/)).toBeInTheDocument();
  });

  it("erro ao salvar fica no modal", async () => {
    const hub = open(closed());
    hub.saveProjectSettings.mockRejectedValueOnce(new Error("host inválido"));
    fireEvent.click(await screen.findByRole("button", { name: "Salvar" }));
    expect(await screen.findByText("host inválido")).toBeInTheDocument();
    expect(useAppStore.getState().projectSettings).not.toBeNull();
  });

  it("lista do PC (F9.11): fica no host, só texto exato, e salva à parte do hubd", async () => {
    vi.mocked(pcAllowedCommands).mockResolvedValueOnce(["git push"]);
    const hub = open(closed());
    const lista = await screen.findByRole("region", { name: "Comandos sem clique no PC" });
    expect(pcAllowedCommands).toHaveBeenCalledWith("pessoal", "game");
    expect(within(lista).getByText("git push")).toBeInTheDocument();
    expect(within(lista).getByText(/texto exato/)).toBeInTheDocument();
    expect(within(lista).getByText(/Fica no PC, não na VM/)).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Novo em Comandos sem clique no PC"), {
      target: { value: "git pull" },
    });
    fireEvent.click(screen.getByTitle("Adicionar em Comandos sem clique no PC"));
    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await vi.waitFor(() => expect(useAppStore.getState().projectSettings).toBeNull());
    expect(setPcAllowedCommands).toHaveBeenCalledWith("pessoal", "game", ["git push", "git pull"]);
    // Nada da lista do PC vai para o hubd (VM).
    expect(hub.saveProjectSettings).toHaveBeenCalledWith(
      "pessoal",
      "game",
      expect.not.objectContaining({ pc_commands: expect.anything() }),
    );
  });

  it("lista do PC sem mudança não regrava; fora do app desktop nem aparece", async () => {
    vi.mocked(setPcAllowedCommands).mockClear();
    open(closed());
    await screen.findByRole("checkbox", { name: /Docker/ });
    expect(screen.queryByRole("region", { name: "Comandos sem clique no PC" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await vi.waitFor(() => expect(useAppStore.getState().projectSettings).toBeNull());
    expect(setPcAllowedCommands).not.toHaveBeenCalled();
  });
});
