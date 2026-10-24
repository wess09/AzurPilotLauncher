import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { acceptSnapshot, type Snapshot } from "./types";
import { translator, localeKeys } from "./i18n";
import dictionaries from "virtual:launcher-locales";

const bridge = vi.hoisted(() => ({
  command: vi.fn(),
  receive: (_s: Snapshot) => {},
  close: () => {},
}));
const ready = (): Snapshot => ({
  sequence: 1,
  task_id: 1,
  phase: "idle",
  operation: null,
  update_version: null,
  update_checked: true,
  backend_running: false,
  progress: null,
  error: null,
  error_code: null,
  connection_error: null,
  message: null,
  can_start: true,
  can_repair: true,
  can_stop: false,
  can_check: true,
  can_update: false,
  can_cancel: false,
});
vi.mock("./bridge", () => ({
  native: false,
  command: bridge.command,
  getContext: async () => ({
    locale: "en",
    version: "2.2.5",
    platform: "windows",
  }),
  subscribe: async (receive: (s: Snapshot) => void, close: () => void) => {
    bridge.receive = receive;
    bridge.close = close;
    receive(ready());
    return () => {};
  },
}));
beforeEach(() => {
  bridge.command.mockReset().mockResolvedValue(undefined);
  window.matchMedia = vi
    .fn()
    .mockReturnValue({
      matches: true,
      addEventListener() {},
      removeEventListener() {},
    });
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  HTMLMediaElement.prototype.pause = vi.fn();
  HTMLMediaElement.prototype.play = vi.fn().mockResolvedValue(undefined);
});
afterEach(cleanup);
const mount = async () => {
  render(<App />);
  await screen.findByRole("button", { name: /Launch$/ });
};
const send = (s: Partial<Snapshot>) =>
  act(() => bridge.receive({ ...ready(), sequence: 2, ...s }));

describe("launcher actions", () => {
  it("launches only when the Rust snapshot permits it", async () => {
    await mount();
    fireEvent.click(screen.getByRole("button", { name: /Launch$/ }));
    expect(bridge.command).toHaveBeenCalledWith("launcher_start");
    await send({
      phase: "starting",
      can_start: false,
      can_repair: false,
      can_check: false,
      can_cancel: true,
    });
    expect(
      screen.getByRole("button", { name: /Starting/ }).hasAttribute("disabled"),
    ).toBe(false);
    expect(
      screen
        .getByRole("button", { name: /Repair environment/ })
        .hasAttribute("disabled"),
    ).toBe(true);
    expect(screen.getByText(/Click to cancel/)).toBeTruthy();
  });
  it("offers an explicit update and blocks launch and repair", async () => {
    await mount();
    await send({
      phase: "update_available",
      update_version: "3.0.0",
      can_update: true,
      can_start: false,
      can_repair: false,
    });
    fireEvent.click(screen.getByRole("button", { name: /Update launcher/ }));
    expect(bridge.command).toHaveBeenCalledWith("launcher_install_update");
    expect(screen.queryByRole("button", { name: /Launch$/ })).toBeNull();
  });
  it("retries an update check rather than bypassing its failure", async () => {
    await mount();
    await send({
      phase: "error",
      operation: "check",
      update_checked: false,
      error: "Offline",
      can_start: false,
      can_repair: false,
    });
    fireEvent.click(screen.getByRole("button", { name: /Check again/ }));
    expect(bridge.command).toHaveBeenCalledWith("launcher_check_update");
  });
  it("confirms repair and never starts the service as a side effect", async () => {
    await mount();
    await send({
      phase: "error",
      operation: "repair",
      error: "venv broken",
      can_start: false,
      can_repair: true,
    });
    fireEvent.click(screen.getByRole("button", { name: /Retry repair/ }));
    expect(bridge.command).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Start repair" }));
    await waitFor(() =>
      expect(bridge.command).toHaveBeenCalledWith("launcher_repair"),
    );
    expect(bridge.command).not.toHaveBeenCalledWith("launcher_start");
  });
  it("keeps repair disabled until the running service is stopped", async () => {
    await mount();
    await send({
      phase: "running",
      backend_running: true,
      can_start: false,
      can_repair: false,
      can_stop: true,
    });
    expect(
      screen
        .getByRole("button", { name: /Repair environment/ })
        .hasAttribute("disabled"),
    ).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: /Stop service/ }));
    expect(bridge.command).toHaveBeenCalledWith("launcher_stop");
  });
  it("cancels a running task by clicking the launch button twice", async () => {
    await mount();
    await send({
      phase: "starting",
      can_start: false,
      can_cancel: true,
      can_repair: false,
    });
    const launch = screen.getByRole("button", { name: /Starting/ });
    fireEvent.click(launch);
    expect(bridge.command).not.toHaveBeenCalled();
    const armed = screen.getByRole("button", {
      name: /Click again to cancel/,
    });
    fireEvent.click(armed);
    expect(bridge.command).toHaveBeenCalledWith("launcher_cancel");
  });
  it("does not cancel a task until the exit confirmation is accepted", async () => {
    await mount();
    act(() => bridge.close());
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(bridge.command).not.toHaveBeenCalled();
    act(() => bridge.close());
    fireEvent.click(
      screen.getByRole("button", { name: "Cancel task and exit" }),
    );
    expect(bridge.command).toHaveBeenCalledWith("launcher_cancel_and_exit");
  });
});

describe("snapshot ordering and progress", () => {
  it("accepts the newest snapshot and rejects stale handshakes, duplicates and invalid sequence values", () => {
    const current = { ...ready(), sequence: 4 };
    for (const sequence of [0, 2, 4, NaN, Infinity, -1, 0.5])
      expect(acceptSnapshot(current, { ...ready(), sequence })).toBe(current);
    const next = { ...ready(), sequence: 5 };
    expect(acceptSnapshot(current, next)).toBe(next);
  });
  it("shows the current step on the launch button without raw output", async () => {
    await mount();
    await send({
      phase: "starting",
      can_start: false,
      can_cancel: true,
      progress: {
        title: "Preparing",
        detail: "FETCH REPOSITORY BRANCH",
        subtitle: "",
        progress: 80.5,
        uv_progress: { progress: 65, detail: "Dependencies", task_id: 1 },
        is_error: false,
        terminal: false,
        sequence: 1,
      },
    });
    const button = screen.getByRole("button", { name: "Preparing" });
    expect(button.hasAttribute("disabled")).toBe(false);
    expect(screen.getByText(/Click to cancel/)).toBeTruthy();
    expect(screen.queryByText(/FETCH/)).toBeNull();
    expect(screen.queryByText(/Dependencies/)).toBeNull();
  });
  it("shows cancelling instead of the stale step after cancellation", async () => {
    await mount();
    await send({
      phase: "cancelling",
      can_start: false,
      can_cancel: true,
      progress: {
        title: "Preparing",
        detail: "FETCH REPOSITORY BRANCH",
        subtitle: "",
        progress: 42,
        is_error: false,
        terminal: false,
        sequence: 2,
      },
    });
    expect(screen.getByRole("button", { name: "Cancelling" })).toBeTruthy();
    expect(screen.queryByText("Preparing")).toBeNull();
  });
});

it("uses the same complete translation keys in all four languages", () => {
  for (const locale of ["zh-CN", "zh-TW", "ja", "en"]) {
    expect(Object.keys(dictionaries[locale]).sort()).toEqual(
      [...localeKeys].sort(),
    );
    for (const key of localeKeys) expect(translator(locale)(key)).not.toBe(key);
    expect(translator(locale)("update_hint", { version: "3.0.0" })).toContain(
      "3.0.0",
    );
  }
});
