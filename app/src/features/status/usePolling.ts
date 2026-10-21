import { useEffect } from "react";
import { hubConnection, hubFetch, vmStatus } from "../../bridge/host";
import { HubClient } from "../../bridge/hub";
import { useAppStore } from "../../store";
import { reloadPage, rememberUiUpdate, shortUiHash, uiChanged } from "./uiUpdate";

const EVERY_MS = 5000;
/** Tempo do aviso antes do reload: a página nova chega do hubd e os terminais reconectam. */
const RELOAD_MS = 1_200;

/** Conecta ao hubd e atualiza saúde, VM, notebook e perfis a cada 5 s. */
export function usePolling() {
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    /** Commit da UI que já estava em uso; `null` até a primeira resposta com UI servida. */
    let lastUi: string | null = null;
    /** Quantas vezes já esperamos o atualizador terminar antes de recarregar. */
    let esperas = 0;

    async function tick() {
      const set = useAppStore.setState;
      let { hub } = useAppStore.getState();
      try {
        if (!hub) {
          hub = new HubClient(await hubConnection(), hubFetch);
          set({ hub });
        }
        const [health, vm] = await Promise.all([hub.health(), vmStatus()]);
        // F8.7: o atualizador trocou a UI em `versoes/atual`; recarrega sem fechar o app.
        const currentUi = health.commit.ui;
        if (currentUi && uiChanged(lastUi, currentUi)) {
          // O atualizador troca os links antes de reiniciar o hubd: espera ele terminar para o
          // reload não cair no buraco do restart (e para o "Voltar" cancelar a troca).
          let rodando = false;
          let shell = false;
          try {
            const status = await hub.updateStatus();
            rodando = status.running;
            shell = status.shell_changed === true;
          } catch {
            // hubd fora do ar durante o reinício: recarrega mesmo assim; a página volta quando
            // ele subir e o próximo ciclo confere de novo.
          }
          if (rodando && esperas < 30) {
            esperas += 1;
            if (!stopped) timer = setTimeout(tick, 1_000);
            return;
          }
          rememberUiUpdate({ commit: currentUi, shell });
          set({ toast: `Versão nova aplicada (${shortUiHash(currentUi)}): recarregando…` });
          stopped = true;
          setTimeout(reloadPage, RELOAD_MS);
          return;
        }
        esperas = 0;
        lastUi = currentUi ?? lastUi;
        const [notebook, memory, profiles, sessions] = await Promise.all([
          hub.notebook(),
          hub.memory(),
          hub.profiles(),
          hub.sessions(),
        ]);
        const { activeProfile, closedTabs } = useAppStore.getState();
        // Perfil restaurado do sessionStorage (F8.2) pode ter saído da config: valida.
        const profileKnown = activeProfile !== null && profiles.some((p) => p.id === activeProfile);
        set({
          health,
          vm,
          notebook,
          memory,
          profiles,
          sessions,
          hubError: null,
          activeProfile: profileKnown ? activeProfile : (profiles[0]?.id ?? null),
          ...(profileKnown ? {} : { activeProject: null, projects: [] }),
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
