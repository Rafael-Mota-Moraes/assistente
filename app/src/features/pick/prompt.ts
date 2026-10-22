/** O pedido que vai para o agente (F12.3/F12.4).
 *
 * Monta o texto curto com o que o usuário apontou: URL, onde está, seletor, texto e o pedido.
 * Em dev, o "onde" é o arquivo:linha do JSX; sem fonte (build de produção) o pick leva o
 * componente, o seletor e o texto, e o pedido manda o agente procurar o arquivo antes de
 * editar em vez de chutar. */
import { describeWhere, resolveCodeContext, searchInstruction } from "./context";
import type { PickElement } from "./types";

/** Texto do pedido que o agente recebe. */
export function buildPickPrompt(
  pick: PickElement,
  request: string,
  projectRoot?: string | null,
): string {
  const context = resolveCodeContext(pick.code, projectRoot);
  const lines = [`Apontei este elemento da página ${pick.url}:`, ""];
  lines.push(`- Onde: ${describeWhere(context, pick)}`);
  if (pick.selector) lines.push(`- Seletor: ${pick.selector}`);
  if (pick.text) lines.push(`- Texto: "${pick.text}"`);
  if (!context.source) {
    lines.push("");
    lines.push(searchInstruction(pick, context));
  }
  lines.push("");
  lines.push(`Pedido: ${request}`);
  return lines.join("\n");
}
