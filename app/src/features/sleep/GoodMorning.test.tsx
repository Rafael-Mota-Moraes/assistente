import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { type HubClient, HubError } from "../../bridge/hub";
import type { CollectReport } from "../../bridge/types";
import { useAppStore } from "../../store";
import { GoodMorning } from "./GoodMorning";
import { mergeMorning, openMorning, yesterday } from "./morning";

const report: CollectReport = {
  base: "main",
  sleep_branch: "dorme/2026-10-19",
  commits: [
    { hash: "abc1234def", subject: "merge: f9.9 (modo-dormir)" },
    { hash: "9876543cba", subject: "merge: f1.2 (modo-dormir)" },
  ],
  relatorio: "Fila esgotada para o agente.",
  fast_forward: true,
};

function fakeHub(overrides: Partial<HubClient> = {}) {
  return {
    collectSleep: vi.fn(async () => report),
    mergeSleep: vi.fn(async () => {}),
    ...overrides,
  } as unknown as HubClient;
}

function open(hub: HubClient) {
  useAppStore.setState({ hub, activeProfile: "pessoal", activeProject: "game" });
}

describe("GoodMorning", () => {
  it("recolhe a noite de ontem e mostra o que foi feito e as falhas", async () => {
    const hub = fakeHub();
    open(hub);
    render(<GoodMorning />);
    await openMorning();

    expect(hub.collectSleep).toHaveBeenCalledWith({
      profile: "pessoal",
      project: "game",
      date: yesterday(),
    });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(/merge: f9\.9 \(modo-dormir\)/)).toBeInTheDocument();
    expect(screen.getByText(/merge: f1\.2 \(modo-dormir\)/)).toBeInTheDocument();
    expect(screen.getByText("RELATORIO.md")).toBeInTheDocument();
    expect(screen.getByText("Fila esgotada para o agente.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Mesclar em main/ })).toBeEnabled();
  });

  it("merge com um clique marca como mesclado", async () => {
    const hub = fakeHub();
    open(hub);
    render(<GoodMorning />);
    await openMorning();

    fireEvent.click(screen.getByRole("button", { name: /Mesclar em main/ }));
    expect(await screen.findByText(/Mesclado em main/)).toBeInTheDocument();
    expect(hub.mergeSleep).toHaveBeenCalledWith({
      profile: "pessoal",
      project: "game",
      date: yesterday(),
    });
    // O botão some depois de mesclar.
    expect(screen.queryByRole("button", { name: /Mesclar em main/ })).not.toBeInTheDocument();
  });

  it("merge recusado (409) mostra o motivo do hubd", async () => {
    const hub = fakeHub({
      mergeSleep: vi.fn(async () => {
        throw new HubError(409, "merge recusado: Not possible to fast-forward");
      }),
    } as Partial<HubClient>);
    open(hub);
    render(<GoodMorning />);
    await openMorning();

    await mergeMorning();
    expect(await screen.findByText(/Not possible to fast-forward/)).toBeInTheDocument();
    expect(screen.queryByText(/Mesclado em main/)).not.toBeInTheDocument();
  });

  it("sem fast-forward: avisa para mesclar à mão, sem botão", async () => {
    open(
      fakeHub({
        collectSleep: vi.fn(async () => ({ ...report, fast_forward: false })),
      } as Partial<HubClient>),
    );
    render(<GoodMorning />);
    await openMorning();

    expect(await screen.findByText(/A base andou durante a noite/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Mesclar/ })).not.toBeInTheDocument();
  });

  it("falha ao recolher mostra o erro com tentativa de novo", async () => {
    open(
      fakeHub({
        collectSleep: vi.fn(async () => {
          throw new HubError(502, "não consegui recolher do notebook: ssh: notebook offline");
        }),
      } as Partial<HubClient>),
    );
    render(<GoodMorning />);
    await openMorning();

    expect(await screen.findByText(/notebook offline/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Recolher/ })).toBeEnabled();
  });

  it("sem nada recolhido ainda, não abre", async () => {
    render(<GoodMorning />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("fechar limpa o estado", async () => {
    open(fakeHub());
    render(<GoodMorning />);
    await openMorning();
    fireEvent.click(screen.getByTitle("Fechar"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(useAppStore.getState().morning).toBeNull();
  });
});
