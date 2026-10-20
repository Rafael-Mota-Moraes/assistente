import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { HubClient } from "../../bridge/hub";
import type { ProjectSettingsView } from "../../bridge/types";
import { useAppStore } from "../../store";
import { ProjectSettingsDialog } from "./ProjectSettingsDialog";

const closed = (): ProjectSettingsView => ({
  settings: { docker: false, allow_hosts: [], full_network: false, allowed_commands: [] },
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
    });
    expect(useAppStore.getState().toast).toMatch(/próximas sessões/);
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
});
