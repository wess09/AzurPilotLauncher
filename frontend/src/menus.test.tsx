// 托盘自绘菜单与页面内右键菜单的渲染与交互测试。
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TrayMenu } from "./TrayMenu";

const bridge = vi.hoisted(() => ({
  native: false,
  instances: [] as { name: string; status: string }[],
  command: vi.fn(async (name: string) => {
    if (name === "tray_instances") return bridge.instances;
    if (name === "settings_get") return { reducedMotion: false };
    return undefined;
  }),
  getContext: vi.fn(async () => ({
    locale: "zh-CN",
    version: "3.0.7",
    platform: "windows",
  })),
}));
vi.mock("./bridge", () => ({
  native: bridge.native,
  command: bridge.command,
  getContext: bridge.getContext,
}));

afterEach(cleanup);

describe("TrayMenu", () => {
  beforeEach(() => {
    bridge.command.mockClear();
    bridge.instances = [];
  });

  it("渲染三个菜单项，点击回传动作命令", async () => {
    render(<TrayMenu />);
    expect(await screen.findByRole("menuitem", { name: "首页" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "显示 / 隐藏" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "退出" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "退出" }));
    expect(bridge.command).toHaveBeenCalledWith("tray_menu_action", { id: "quit" });
  });

  it("Esc 调用收起命令", async () => {
    render(<TrayMenu />);
    await screen.findByRole("menuitem", { name: "首页" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(bridge.command).toHaveBeenCalledWith("tray_menu_hide");
  });

  it("按实例状态渲染启停项与全部启停，点击回传调度动作", async () => {
    bridge.instances = [
      { name: "alas", status: "running" },
      { name: "alas2", status: "stopped" },
    ];
    render(<TrayMenu />);
    expect(await screen.findByRole("menuitem", { name: /alas.*运行中/ })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: /alas2.*已停止/ })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "启动全部" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "停止全部" })).toBeTruthy();

    fireEvent.click(screen.getByRole("menuitem", { name: /alas.*运行中/ }));
    expect(bridge.command).toHaveBeenCalledWith("tray_menu_action", {
      id: "scheduler:stop:alas",
    });
    fireEvent.click(screen.getByRole("menuitem", { name: /alas2.*已停止/ }));
    expect(bridge.command).toHaveBeenCalledWith("tray_menu_action", {
      id: "scheduler:start:alas2",
    });
    fireEvent.click(screen.getByRole("menuitem", { name: "启动全部" }));
    expect(bridge.command).toHaveBeenCalledWith("tray_menu_action", {
      id: "scheduler:start-all",
    });
    fireEvent.click(screen.getByRole("menuitem", { name: "停止全部" }));
    expect(bridge.command).toHaveBeenCalledWith("tray_menu_action", {
      id: "scheduler:stop-all",
    });
  });
});
