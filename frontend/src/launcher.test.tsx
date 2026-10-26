import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { acceptSnapshot, type Snapshot } from "./types";
import { translator, localeKeys } from "./i18n";
import dictionaries from "virtual:launcher-locales";

const bridge = vi.hoisted(() => ({
  command: vi.fn(),
  getContext: vi.fn(),
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
  deployment_required: false,
  progress: null,
  error: null,
  error_code: null,
  connection_error: null,
  message: null,
  can_start: true,
  can_deploy: false,
  can_repair: true,
  can_stop: false,
  can_check: true,
  can_update: false,
  can_cancel: false,
});
vi.mock("./bridge", () => ({
  native: false,
  command: bridge.command,
  subscribeLogArchiveProgress: async () => () => {},
  getContext: bridge.getContext,
  subscribe: async (receive: (s: Snapshot) => void, close: () => void) => {
    bridge.receive = receive;
    bridge.close = close;
    receive(ready());
    return () => {};
  },
}));
beforeEach(() => {
  bridge.command.mockReset().mockResolvedValue(undefined);
  bridge.getContext.mockReset().mockResolvedValue({
    locale: "en",
    version: "2.2.5",
    platform: "windows",
  });
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
  await screen.findByRole("button", { name: /Run$/ });
};
const send = (s: Partial<Snapshot>) =>
  act(() => bridge.receive({ ...ready(), sequence: 2, ...s }));

describe("launcher actions", () => {
  it("keeps initialization errors visible after state pushes", async () => {
    bridge.getContext.mockRejectedValueOnce("Context permission denied");
    render(<App />);
    expect(await screen.findByText("Context permission denied")).toBeTruthy();
    await send({ sequence: 3 });
    expect(screen.getByText("Context permission denied")).toBeTruthy();
  });
  it("reports a settings save failure and rolls the switch back", async () => {
    const settings = { launchOnStartup: false, autoStartService: false, keepWebviewOnTray: false, reducedMotion: false, preventSleep: false, audioEnabled: false, updateChannel: "stable", updateSource: "auto", language: null, downloadSource: "auto", pythonSource: "auto", pypiSource: "auto" };
    bridge.command.mockImplementation(async (name: string) => {
      if (name === "settings_get") return settings;
      if (name === "settings_set") throw new Error("Settings permission denied");
    });
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const toggle = await screen.findByRole("switch", { name: "Start the service when the launcher starts" });
    fireEvent.click(toggle);
    expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Error: Settings permission denied");
    expect(toggle).toHaveProperty("checked", false);
  });
  it("saves named mirrors without showing URL paths", async () => {
    const settings = { launchOnStartup: false, autoStartService: false, keepWebviewOnTray: false, reducedMotion: false, preventSleep: false, audioEnabled: false, updateChannel: "stable", updateSource: "auto", language: null, downloadSource: "auto", pythonSource: "auto", pypiSource: "auto" };
    bridge.command.mockImplementation(async (name: string, args?: { settings: unknown }) => name === "settings_set" ? args?.settings : name === "settings_get" ? settings : undefined);
    await mount();
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const python = await screen.findByRole("combobox", { name: "Python source" });
    fireEvent.click(python);
    const options = await screen.findAllByRole("option");
    expect(options.every(option => !option.textContent?.includes("http"))).toBe(true);
    fireEvent.click(screen.getByRole("option", { name: "Taobao mirror" }));
    await waitFor(() => expect(bridge.command).toHaveBeenCalledWith("settings_set", { settings: { ...settings, pythonSource: "npmmirror" } }));
  });
  it("pauses the background video when reduced motion is enabled", async () => {
    const settings = { launchOnStartup: false, autoStartService: false, keepWebviewOnTray: false, reducedMotion: false, preventSleep: false, audioEnabled: false, updateChannel: "stable", updateSource: "auto", language: null, downloadSource: "auto", pythonSource: "auto", pypiSource: "auto" };
    bridge.command.mockImplementation(async (name: string, args?: { settings: unknown }) => name === "settings_set" ? args?.settings : name === "settings_get" ? settings : undefined);
    // 覆写系统偏好为“未减弱动态”，验证视频暂停完全由启动器设置驱动。
    window.matchMedia = vi.fn().mockReturnValue({
      matches: false,
      addEventListener() {},
      removeEventListener() {},
    });
    const pause = vi.fn();
    HTMLMediaElement.prototype.pause = pause;
    await mount();
    expect(pause).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const toggle = await screen.findByRole("switch", { name: "Reduce motion" });
    fireEvent.click(toggle);
    await waitFor(() => expect(bridge.command).toHaveBeenCalledWith("settings_set", { settings: { ...settings, reducedMotion: true } }));
    await waitFor(() => expect(pause).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("switch", { name: "Prevent automatic sleep" }));
    await waitFor(() => expect(bridge.command).toHaveBeenCalledWith("settings_set", { settings: { ...settings, reducedMotion: true, preventSleep: true } }));
  });
  it("pauses hidden-window video and resumes it on restore", async () => {
    window.matchMedia = vi.fn().mockReturnValue({
      matches: false,
      addEventListener() {},
      removeEventListener() {},
    });
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(false);
    await mount();
    const video = document.querySelector("video")!;
    // 播放完全由可见性及偏好控制，避免隐藏启动时 autoplay 抢先播放。
    expect(video.autoplay).toBe(false);
    expect(video.play).toHaveBeenCalled();
    expect(video.pause).not.toHaveBeenCalled();

    hidden.mockReturnValue(true);
    fireEvent(document, new Event("visibilitychange"));
    expect(video.pause).toHaveBeenCalledTimes(1);
    vi.mocked(video.play).mockClear();

    hidden.mockReturnValue(false);
    fireEvent(document, new Event("visibilitychange"));
    expect(video.play).toHaveBeenCalledTimes(1);
    expect(video.muted).toBe(true);
  });
  it("keeps initially hidden video paused and honors motion preferences on restore", async () => {
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    let onMotionChange = () => {};
    const motion = {
      matches: false,
      addEventListener: (_name: string, listener: () => void) => { onMotionChange = listener; },
      removeEventListener() {},
    };
    window.matchMedia = vi.fn().mockReturnValue(motion);
    await mount();
    const video = document.querySelector("video")!;
    expect(video.play).not.toHaveBeenCalled();
    expect(video.pause).toHaveBeenCalled();

    motion.matches = true;
    hidden.mockReturnValue(false);
    fireEvent(document, new Event("visibilitychange"));
    expect(video.play).not.toHaveBeenCalled();

    motion.matches = false;
    act(() => onMotionChange());
    expect(video.play).toHaveBeenCalledTimes(1);
  });
  it("does not resume hidden video after changing audio or reduced motion settings", async () => {
    const settings = { launchOnStartup: false, autoStartService: false, keepWebviewOnTray: true, reducedMotion: true, preventSleep: false, audioEnabled: false, updateChannel: "stable", updateSource: "auto", language: null, downloadSource: "auto", pythonSource: "auto", pypiSource: "auto" };
    bridge.command.mockImplementation(async (name: string, args?: { settings: unknown }) => name === "settings_set" ? args?.settings : name === "settings_get" ? settings : undefined);
    window.matchMedia = vi.fn().mockReturnValue({
      matches: false,
      addEventListener() {},
      removeEventListener() {},
    });
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    await mount();
    const video = document.querySelector("video")!;
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const toggle = await screen.findByRole("switch", { name: "Reduce motion" });
    fireEvent.click(toggle);
    await waitFor(() => expect(bridge.command).toHaveBeenCalledWith("settings_set", { settings: { ...settings, reducedMotion: false } }));
    await waitFor(() => expect(document.documentElement.classList.contains("reduced-motion")).toBe(false));
    fireEvent.click(screen.getByRole("switch", { name: "Play background music" }));
    await waitFor(() => expect(video.muted).toBe(false));
    expect(video.play).not.toHaveBeenCalled();

    hidden.mockReturnValue(false);
    fireEvent(document, new Event("visibilitychange"));
    expect(video.play).toHaveBeenCalledTimes(1);
    expect(video.muted).toBe(false);
    fireEvent.click(toggle);
    await waitFor(() => expect(document.documentElement.classList.contains("reduced-motion")).toBe(true));
    vi.mocked(video.play).mockClear();
    hidden.mockReturnValue(true);
    fireEvent(document, new Event("visibilitychange"));
    hidden.mockReturnValue(false);
    fireEvent(document, new Event("visibilitychange"));
    expect(video.play).not.toHaveBeenCalled();
  });
  it("shows Deploy for missing components and Run after deployment", async () => {
    await mount();
    await send({ deployment_required: true, can_start: false, can_deploy: true });
    fireEvent.click(screen.getByRole("button", { name: /Deploy$/ }));
    expect(bridge.command).toHaveBeenCalledWith("launcher_deploy");
    expect(bridge.command).not.toHaveBeenCalledWith("launcher_start");
    expect(screen.getByRole("button", { name: /Deploy$/ }).getAttribute("title")).toBe("Download missing components. Click Run after deployment finishes.");
    await send({ sequence: 3, deployment_required: false });
    expect(screen.getByRole("button", { name: /Run$/ })).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Deploy$/ })).toBeNull();
    expect(bridge.command).not.toHaveBeenCalledWith("launcher_start");
    fireEvent.click(screen.getByRole("button", { name: /Run$/ }));
    await waitFor(() => expect(bridge.command).toHaveBeenCalledWith("launcher_start"));
  });
  it("launches only when the Rust snapshot permits it", async () => {
    await mount();
    fireEvent.click(screen.getByRole("button", { name: /Run$/ }));
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
    expect(screen.queryByRole("button", { name: /Run$/ })).toBeNull();
  });
  it.each(["launcher_start", "launcher_install_update"])(
    "shows a rejected %s command, keeps the error across snapshots and permits retry",
    async (name) => {
      await mount();
      const updating = name === "launcher_install_update";
      const state = updating
        ? { phase: "update_available" as const, update_version: "3.0.4", can_update: true, can_start: false }
        : {};
      if (updating) await send(state);
      bridge.command.mockRejectedValueOnce("This operation is only allowed from a local launcher page.");
      const label = updating ? /Update launcher/ : /Run$/;
      fireEvent.click(screen.getByRole("button", { name: label }));
      expect(await screen.findByText("This operation is only allowed from a local launcher page.")).toBeTruthy();
      await send({ ...state, sequence: 3 });
      expect(screen.getByText("This operation is only allowed from a local launcher page.")).toBeTruthy();
      const errorDialog = screen.getByRole("alertdialog");
      fireEvent.click(within(errorDialog).getByRole("button", { name: "Close" }));
      fireEvent.click(screen.getByRole("button", { name: label }));
      await waitFor(() => expect(bridge.command.mock.calls.filter(([command]) => command === name)).toHaveLength(2));
      await waitFor(() => expect(screen.queryByText("This operation is only allowed from a local launcher page.")).toBeNull());
    },
  );
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
    expect(bridge.command).not.toHaveBeenCalledWith("launcher_repair");
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
    expect(bridge.command).not.toHaveBeenCalledWith("launcher_cancel");
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
    expect(bridge.command).not.toHaveBeenCalledWith("launcher_cancel_and_exit");
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
