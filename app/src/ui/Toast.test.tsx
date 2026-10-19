import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "../store";
import { Toast } from "./Toast";

describe("Toast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("mostra a mensagem e some sozinha depois de alguns segundos", () => {
    useAppStore.setState({ toast: "não consegui abrir o Cursor: cursor não instalado" });
    render(<Toast />);
    expect(screen.getByRole("status")).toHaveTextContent("não consegui abrir o Cursor");

    act(() => {
      vi.advanceTimersByTime(6000);
    });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(useAppStore.getState().toast).toBeNull();
  });

  it("sem mensagem, não renderiza nada", () => {
    render(<Toast />);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
