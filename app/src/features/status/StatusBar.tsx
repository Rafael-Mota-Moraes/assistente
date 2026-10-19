import type { TunnelStatus, VmState } from "../../bridge/types";
import { useAppStore } from "../../store";
import { StatusDot, type Tone } from "../../ui/StatusDot";

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

export function StatusBar() {
  const { vm, health, hubError, notebook } = useAppStore();
  return (
    <footer className="flex h-8 items-center gap-5 border-t border-line bg-surface-strong px-4 text-xs text-slate-400">
      <StatusDot
        tone={vm ? vmTone[vm.state] : "idle"}
        label={vm ? vmLabel[vm.state] : "VM …"}
        title={vm?.detail ?? undefined}
      />
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
      {notebook?.services.map((service) => (
        <StatusDot
          key={service.name}
          tone={service.up ? "ok" : "off"}
          label={serviceLabel[service.name] ?? service.name}
          title={`${notebook.ssh_host} → 127.0.0.1:${service.port}`}
        />
      ))}
      <span className="ml-auto font-mono text-slate-500">
        {notebook ? `notebook: ${notebook.ssh_host}` : ""}
      </span>
    </footer>
  );
}
