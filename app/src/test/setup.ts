import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";
import { initialState, useAppStore } from "../store";

afterEach(() => {
  cleanup();
  useAppStore.setState(initialState, true);
});
