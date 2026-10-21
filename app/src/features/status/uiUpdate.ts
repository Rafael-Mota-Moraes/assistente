/** F8.7: a UI nova entra em uso sozinha; a casca Tauri (`app/src-tauri`) não — pede reinstalar. */

const KEY = "assistente.ui-update";

export type UiUpdateNotice = {
  commit: string;
  /** O commit aplicado mexeu em `app/src-tauri`: além do reload, reinstale o app. */
  shell: boolean;
};

/** Hashes curtos e completos do mesmo commit contam como iguais (o hubd manda os dois formatos). */
export function sameUiCommit(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  return a.startsWith(b) || b.startsWith(a);
}

/** A UI em uso mudou desde a última consulta ao `/api/health`? */
export function uiChanged(
  previous: string | null | undefined,
  current: string | null | undefined,
): boolean {
  return Boolean(previous && current && !sameUiCommit(previous, current));
}

/** Grava o aviso antes do reload: o sessionStorage sobrevive à recarga da página. */
export function rememberUiUpdate(notice: UiUpdateNotice): void {
  try {
    sessionStorage.setItem(KEY, JSON.stringify(notice));
  } catch {
    // Sem sessionStorage (modo privado/teste): recarrega igual, só não avisa depois.
  }
}

/** Lê o aviso da última atualização; não apaga, para o StrictMode não perdê-lo no duplo mount. */
export function readUiUpdateNotice(): UiUpdateNotice | null {
  let raw: string | null = null;
  try {
    raw = sessionStorage.getItem(KEY);
  } catch {
    return null;
  }
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<UiUpdateNotice>;
    if (typeof parsed.commit !== "string" || !parsed.commit) return null;
    return { commit: parsed.commit, shell: parsed.shell === true };
  } catch {
    return null;
  }
}

/** Apaga o aviso quando o banner some, para um reload manual depois não repeti-lo. */
export function clearUiUpdateNotice(): void {
  try {
    sessionStorage.removeItem(KEY);
  } catch {
    // sem sessionStorage: nada a limpar.
  }
}

/** Hash curto para o aviso, sem estourar a largura. */
export function shortUiHash(commit: string): string {
  return commit.length > 10 ? commit.slice(0, 10) : commit;
}

/** Recarrega a página; separado para o teste trocar por um espião. */
export function reloadPage(): void {
  window.location.reload();
}
