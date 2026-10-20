import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type HubClient, HubError } from "../../bridge/hub";
import { useAppStore } from "../../store";
import { SleepButton } from "./SleepButton";

const started = {
  date: "2026-10-20",
  session: "dorme-game",
  remote_dir: "~/assistente/projetos/pessoal/game",
  prompt_file: "~/assistente/briefings/game.md",
  log_file: "~/assistente/logs/game.log",
};

function fakeHub(overrides: Partial<HubClient> = {}) {
  return {
    startSleep: vi.fn(async () => started),
    stopSleep: vi.fn(async () => {}),
    sleepStatus: vi.fn(async () => ({ sleeping: false, log_tail: null })),
    ...overrides,
  } as unknown as HubClient;
}

function connect(hub: HubClient) {
  useAppStore.setState({ hub, activeProfile: "pessoal", activeProject: "game" });
}

describe("SleepButton", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("começa a dormir e avisa no toast", async () => {
    const hub = fakeHub();
    connect(hub);
    render(<SleepButton />);

    fireEvent.click(await screen.findByRole("button", { name: /Hora de dormir/ }));
    await waitFor(() => expect(useAppStore.getState().toast).toContain("noite de 2026-10-20"));
    expect(hub.startSleep).toHaveBeenCalledWith({ profile: "pessoal", project: "game" });
  });

  it("árvore suja: confirmação aceita dorme com WIP", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const startSleep = vi
      .fn()
      .mockRejectedValueOnce(new HubError(409, "árvore suja — commite as mudanças"))
      .mockResolvedValueOnce(started);
    connect(fakeHub({ startSleep } as unknown as Partial<HubClient>));
    render(<SleepButton />);

    fireEvent.click(await screen.findByRole("button", { name: /Hora de dormir/ }));
    await waitFor(() => expect(useAppStore.getState().toast).toContain("com commit WIP"));
    expect(confirm).toHaveBeenCalled();
    expect(startSleep).toHaveBeenLastCalledWith({
      profile: "pessoal",
      project: "game",
      wip: true,
    });
  });

  it("árvore suja: sem confirmação não dorme", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(false);
    const startSleep = vi
      .fn()
      .mockRejectedValue(new HubError(409, "árvore suja — commite as mudanças"));
    connect(fakeHub({ startSleep } as unknown as Partial<HubClient>));
    render(<SleepButton />);

    fireEvent.click(await screen.findByRole("button", { name: /Hora de dormir/ }));
    await waitFor(() => expect(startSleep).toHaveBeenCalledTimes(1));
    expect(useAppStore.getState().toast).toBeNull();
  });

  it("erro do hubd aparece no toast", async () => {
    const startSleep = vi
      .fn()
      .mockRejectedValueOnce(new HubError(502, "não consegui iniciar o modo dormir no notebook"));
    connect(fakeHub({ startSleep } as unknown as Partial<HubClient>));
    render(<SleepButton />);

    fireEvent.click(await screen.findByRole("button", { name: /Hora de dormir/ }));
    await waitFor(() => expect(useAppStore.getState().toast).toContain("não consegui iniciar"));
  });

  it("dormindo: botão vira Dormindo… com a tentativa e para sob confirmação", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const hub = fakeHub({
      sleepStatus: vi.fn(async () => ({
        sleeping: true,
        log_tail: "[2026-10-20 02:00:00] tarefa: tentativa 2/3 (timeout 240m)",
      })),
    } as unknown as Partial<HubClient>);
    connect(hub);
    render(<SleepButton />);

    fireEvent.click(await screen.findByRole("button", { name: /Dormindo… \(tentativa 2\/3\)/ }));
    await waitFor(() => expect(useAppStore.getState().toast).toContain("Parada pedida"));
    expect(confirm).toHaveBeenCalled();
    expect(hub.stopSleep).toHaveBeenCalled();
  });
});
