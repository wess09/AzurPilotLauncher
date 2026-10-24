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
import { advance, channel, target } from "./progress";
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
    ).toBe(true);
    expect(
      screen
        .getByRole("button", { name: /Repair environment/ })
        .hasAttribute("disabled"),
    ).toBe(true);
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
    fireEvent.click(screen.getByRole("button", { name: /Repair environment/ }));
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
  it("interpolates to the confirmed progress in 250ms and ignores backwards values", () => {
    const c = channel();
    target(c, 60, 0);
    expect(advance(c, 125)).toBe(30);
    expect(advance(c, 250)).toBe(60);
    target(c, 20, 250);
    expect(advance(c, 500)).toBe(60);
  });
  it("retargets from the rendered position without restarting equal targets", () => {
    const c = channel();
    target(c, 40, 0);
    advance(c, 100);
    target(c, 80, 100);
    expect(c.from).toBe(16);
    target(c, 80, 200);
    expect(c.at).toBe(100);
    expect(advance(c, 350)).toBe(80);
  });
  it("settles errors, reduced motion and completion immediately without NaN", () => {
    const c = channel();
    target(c, 42.5, 0, true);
    expect(c.value).toBe(42.5);
    target(c, NaN, 10);
    expect(Number.isFinite(c.value)).toBe(true);
    target(c, 100, 20);
    expect(c.value).toBe(100);
  });
  it("resets bars on a new operation and dependency subtask", async () => {
    await mount();
    const progress = {
      title: "Preparing",
      detail: "",
      subtitle: "",
      progress: 80.5,
      uv_progress: { progress: 65, detail: "Dependencies", task_id: 1 },
      is_error: false,
      terminal: false,
      sequence: 1,
    };
    await send({ phase: "starting", progress, can_start: false });
    expect(
      screen
        .getByRole("progressbar", { name: "Preparing" })
        .getAttribute("aria-valuenow"),
    ).toBe("80");
    await send({
      sequence: 3,
      task_id: 2,
      phase: "repairing",
      progress: {
        ...progress,
        progress: 5,
        uv_progress: { ...progress.uv_progress, progress: 2, task_id: 2 },
      },
      can_start: false,
    });
    expect(
      screen
        .getByRole("progressbar", { name: "Preparing" })
        .getAttribute("aria-valuenow"),
    ).toBe("5");
    expect(
      screen
        .getByRole("progressbar", { name: "Dependencies" })
        .getAttribute("aria-valuenow"),
    ).toBe("2");
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
