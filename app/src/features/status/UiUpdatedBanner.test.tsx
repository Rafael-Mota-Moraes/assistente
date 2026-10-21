import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { UiUpdatedBanner } from "./UiUpdatedBanner";
import { rememberUiUpdate } from "./uiUpdate";

const STORED = "assistente.ui-update";

afterEach(() => {
  sessionStorage.clear();
  vi.useRealTimers();
});

describe("UiUpdatedBanner", () => {
  it("sem atualização pendente não mostra nada", () => {
    const { container } = render(<UiUpdatedBanner />);
    expect(container).toBeEmptyDOMElement();
  });

  it("pede reinstalar o app quando a casca mudou", () => {
    rememberUiUpdate({ commit: "abc1234", shell: true });
    render(<UiUpdatedBanner />);
    expect(screen.getByText(/Versão nova aplicada/)).toBeInTheDocument();
    expect(screen.getByText(/A casca do app mudou/)).toBeInTheDocument();
    expect(screen.getByText("assistente atualizar")).toBeInTheDocument();
  });

  it("com só a UI, o aviso some sozinho e limpa o sessionStorage", () => {
    vi.useFakeTimers();
    rememberUiUpdate({ commit: "abc1234", shell: false });
    render(<UiUpdatedBanner />);
    expect(screen.getByText(/Versão nova aplicada/)).toBeInTheDocument();
    expect(screen.queryByText(/reinstale/i)).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(8_000);
    });
    expect(screen.queryByText(/Versão nova aplicada/)).not.toBeInTheDocument();
    expect(sessionStorage.getItem(STORED)).toBeNull();
  });

  it("dá para fechar o aviso da casca", () => {
    rememberUiUpdate({ commit: "abc1234", shell: true });
    render(<UiUpdatedBanner />);
    fireEvent.click(screen.getByLabelText("Fechar aviso"));
    expect(screen.queryByText(/Versão nova aplicada/)).not.toBeInTheDocument();
    expect(sessionStorage.getItem(STORED)).toBeNull();
  });
});
