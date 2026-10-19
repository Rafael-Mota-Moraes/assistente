import { useEffect } from "react";
import { hubConnection, hubFetch, vmStatus } from "../../bridge/host";
import { HubClient } from "../../bridge/hub";
import { useAppStore } from "../../store";

const EVERY_MS = 5000;

/** Conecta ao hubd e atualiza saúde, VM, notebook e perfis a cada 5 s. */
export function usePolling() {
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    async function tick() {
      const set = useAppStore.setState;
      let { hub } = useAppStore.getState();
      try {
        if (!hub) {
          hub = new HubClient(await hubConnection(), hubFetch);
          set({ hub });
        }
        const [health, vm] = await Promise.all([hub.health(), vmStatus()]);
        const [notebook, memory, profiles, sessions] = await Promise.all([
          hub.notebook(),
          hub.memory(),
          hub.profiles(),
          hub.sessions(),
        ]);
        const { activeProfile, closedTabs } = useAppStore.getState();
        set({
          health,
          vm,
          notebook,
          memory,
          profiles,
          sessions,
          hubError: null,
          activeProfile: activeProfile ?? profiles[0]?.id ?? null,
          // Aba fechada de sessão que morreu (no hubd ou por outro app) sai da lista.
          closedTabs: closedTabs.filter((id) => sessions.some((s) => s.id === id)),
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        set({ health: null, hubError: message, vm: await vmStatus().catch(() => null) });
      }
      if (!stopped) timer = setTimeout(tick, EVERY_MS);
    }

    tick();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, []);
}
