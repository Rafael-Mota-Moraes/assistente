// Pedido para o PC (F9.10): o agente pede (`fora-do-jail --pc`), o hubd guarda e NÃO
// executa nada na VM. Quem executa no PC é o app, depois que você aprovou no painel.
// O app pega no hubd o que foi aprovado (uma vez só), roda o comando na pasta do projeto,
// anota no registro do PC e devolve a saída e o código — o agente recebe pelo long-poll de
// `/v1/exec/{id}`. Sem clique, só o que está na lista exata do projeto, no host (F9.11).

import { hostAutoExec, hostExec } from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import type { ExecResult, ExecView } from "../../bridge/types";

/** Aprova um pedido para o PC, roda no host e devolve o resultado ao hubd. */
export async function approveOnHost(request: ExecView, hub: HubClient): Promise<void> {
  await hub.approveExec(request.id);
  await reportBack(request, hub, () => hostExec(request.id));
}

/** Pedido para o PC na lista exata do projeto (F9.11): quem confere a lista e aprova é o
 * app, no host. Devolve `false` se o comando está fora da lista (o card espera o clique). */
export async function autoRunOnHost(request: ExecView, hub: HubClient): Promise<boolean> {
  const result = await reportBack(request, hub, () => hostAutoExec(request.id));
  return result !== null;
}

async function reportBack<T extends ExecResult | null>(
  request: ExecView,
  hub: HubClient,
  run: () => Promise<T>,
): Promise<T> {
  try {
    const result = await run();
    if (result) await hub.execResult(request.id, result);
    return result;
  } catch (error) {
    // O agente não pode ficar esperando: devolve a falha para o hubd fechar o pedido.
    const message = error instanceof Error ? error.message : String(error);
    await hub.execResult(request.id, { error: message }).catch(() => {});
    throw error;
  }
}
