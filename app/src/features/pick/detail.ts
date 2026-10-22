// O pick que o Rust entrega (F12.2, `PickedElement`) no formato que o hubd recebe (F12.4,
// `PickDetail`). Retângulo, viewport, rolagem e `aria-*` não têm campo próprio no hubd: vão
// no `extra` e ficam no arquivo do pick.
import type { PickDetail, PickedElement } from "../../bridge/types";
import { resolveCodeContext } from "./context";

export function toPickDetail(pick: PickedElement): PickDetail {
  const context = resolveCodeContext(pick.code);
  return {
    url: pick.url,
    selector: pick.selector,
    text: pick.text ?? "",
    html: pick.html ?? "",
    tag: pick.tag,
    id: pick.id,
    classes: pick.classes,
    role: pick.role,
    aria_label: pick.aria.label ?? null,
    dom_path: pick.dom_path,
    aria: pick.aria,
    rect: pick.rect,
    viewport: pick.viewport,
    scroll: pick.scroll,
    component: context.component,
    source: context.source?.label ?? null,
    components: context.stack,
  };
}
