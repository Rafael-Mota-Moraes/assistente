// Pedido para o PC (F9.10): o agente pede (`fora-do-jail --pc`), o hubd guarda e NÃO
// executa nada na VM. Quem executa no PC é o app, depois que você aprovou no painel.
// O app pergunta ao hubd o que foi aprovado, roda o comando na pasta do projeto e devolve
// a saída e o código — o agente recebe pelo long-poll de `/v1/exec/{id}`.

import { hostExec } from "../../bridge/host";
import type { HubClient } from "../../bridge/hub";
import type { ExecView } from "../../bridge/types";

/** Aprova um pedido para o PC, roda no host e devolve o resultado ao hubd. */
export async function approveOnHost(request: ExecView, hub: HubClient): Promise<void> {
  await hub.approveExec(request.id);
  try {
    const result = await hostExec(request.id);
    await hub.execResult(request.id, result);
  } catch (error) {
    // O agente não pode ficar esperando: devolve a falha para o hubd fechar o pedido.
    const message = error instanceof Error ? error.message : String(error);
    await hub.execResult(request.id, { error: message }).catch(() => {});
    throw error;
  }
}
