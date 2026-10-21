import { useState } from "react";
import { vmStart, vmStatus, vmStop } from "../../bridge/host";
import type { MemoryStatus, ServiceStatus, TunnelStatus, VmState } from "../../bridge/types";
import { useAppStore } from "../../store";
import { StatusDot, type Tone } from "../../ui/StatusDot";
import { MemoryStatusPanel } from "../memory/MemoryStatusPanel";

const vmTone: Record<VmState, Tone> = {
  running: "ok",
  stopped: "warn",
  missing: "off",
  "no-incus": "off",
  unknown: "idle",
};

const vmLabel: Record<VmState, string> = {
  running: "VM ligada",
  stopped: "VM parada",
  missing: "VM não criada",
  "no-incus": "sem Incus",
  unknown: "VM ?",
};

const tunnelTone: Record<TunnelStatus["state"], Tone> = {
  up: "ok",
  connecting: "warn",
  down: "off",
};

const serviceLabel: Record<string, string> = { "9router": "9router", ai_memory: "ai-memory" };

/** Tooltip do ponto de serviço: endereço, erro quando caído e hora da última verificação. */
export function serviceTitle(sshHost: string, service: ServiceStatus): string {
  const parts = [`${sshHost} → 127.0.0.1:${service.port}`];
  if (!service.up) parts.push(service.last_error ?? "sem resposta");
  if (service.checked_at > 0)
    parts.push(`verificado às ${new Date(service.checked_at * 1000).toLocaleTimeString("pt-BR")}`);
  return parts.join(" — ");
}

const vmActionClass =
  "rounded border border-line px-1.5 py-0.5 text-[11px] text-slate-300 hover:bg-white/5 disabled:opacity-40";

/** Tooltip do ponto de memória local (F5.0). */
export function memoryTitle(memory: MemoryStatus): string {
  const parts = [`127.0.0.1:${memory.port}`, memory.mirror_dir];
  if (!memory.up) parts.push(memory.last_error ?? "sem resposta");
  if (memory.checked_at > 0)
    parts.push(`verificado às ${new Date(memory.checked_at * 1000).toLocaleTimeString("pt-BR")}`);
  return parts.join(" — ");
}

export function StatusBar() {
  const { vm, health, hubError, notebook, memory } = useAppStore();
  const [vmBusy, setVmBusy] = useState(false);
  const [memoryStatusOpen, setMemoryStatusOpen] = useState(false);

  async function toggleVm(action: "start" | "stop") {
    setVmBusy(true);
    try {
      if (action === "start") await vmStart();
      else await vmStop();
      useAppStore.setState({ vm: await vmStatus() });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      useAppStore.setState({ toast: message });
    } finally {
      setVmBusy(false);
    }
  }

  return (
    <footer className="flex h-8 items-center gap-5 border-t border-line bg-surface-strong px-4 text-xs text-slate-400">
      <span className="flex items-center gap-1.5">
        <StatusDot
          tone={vm ? vmTone[vm.state] : "idle"}
          label={vm ? vmLabel[vm.state] : "VM …"}
          title={vm?.detail ?? undefined}
        />
        {vm?.state === "stopped" && (
          <button
            type="button"
            className={vmActionClass}
            disabled={vmBusy}
            onClick={() => toggleVm("start")}
          >
            {vmBusy ? "…" : "Ligar"}
          </button>
        )}
        {vm?.state === "running" && (
          <button
            type="button"
            className={vmActionClass}
            disabled={vmBusy}
            onClick={() => toggleVm("stop")}
          >
            {vmBusy ? "…" : "Desligar"}
          </button>
        )}
      </span>
      <StatusDot
        tone={health ? "ok" : "off"}
        label={health ? `hubd ${health.version}` : "hubd offline"}
        title={hubError ?? undefined}
      />
      {notebook?.tunnel && (
        <StatusDot
          tone={tunnelTone[notebook.tunnel.state]}
          label="túnel"
          title={notebook.tunnel.last_error ?? `${notebook.ssh_host}: ${notebook.tunnel.state}`}
        />
      )}
      {memory && (
        <button
          type="button"
          onClick={() => setMemoryStatusOpen(true)}
          className="flex items-center"
          title={memoryTitle(memory)}
          data-tone={memory.up ? "ok" : "off"}
        >
          <StatusDot tone={memory.up ? "ok" : "off"} label="ai-memory" />
        </button>
      )}
      {memory && memoryStatusOpen && (
        <MemoryStatusPanel memory={memory} onClose={() => setMemoryStatusOpen(false)} />
      )}
      {notebook?.services
        .filter((service) => service.name !== "ai_memory")
        .map((service) => (
          <StatusDot
            key={service.name}
            tone={service.up ? "ok" : "off"}
            label={serviceLabel[service.name] ?? service.name}
            title={serviceTitle(notebook.ssh_host, service)}
          />
        ))}
      <span className="ml-auto font-mono text-slate-500">
        {notebook ? `notebook: ${notebook.ssh_host}` : ""}
      </span>
    </footer>
  );
}
