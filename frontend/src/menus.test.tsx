// 托盘自绘菜单与页面内右键菜单的渲染与交互测试。
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ContextMenu } from "./ContextMenu";
import { TrayMenu } from "./TrayMenu";

const bridge = vi.hoisted(() => ({
  native: false,
  command: vi.fn(async () => undefined),
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

describe("ContextMenu", () => {
  it("拦截右键默认行为并显示自定义菜单项", () => {
    const { container } = render(<ContextMenu locale="zh-CN" motion={false} />);
    expect(fireEvent.contextMenu(container)).toBe(false);
    expect(screen.getByRole("menuitem", { name: "刷新" })).toBeTruthy();
    // 无选区时复制禁用，无可编辑目标时粘贴/全选禁用。
    expect(screen.getByRole("menuitem", { name: "复制" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("menuitem", { name: "粘贴" }).hasAttribute("disabled")).toBe(true);
  });

  it("Esc 与菜单外按下都会关闭菜单", () => {
    const { container } = render(<ContextMenu locale="zh-CN" motion={false} />);
    fireEvent.contextMenu(container);
    expect(screen.getByRole("menu")).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    fireEvent.contextMenu(container);
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
  });
});

describe("TrayMenu", () => {
  beforeEach(() => {
    bridge.command.mockClear();
  });

  it("渲染三个菜单项，点击回传动作命令", async () => {
    render(<TrayMenu />);
    expect(await screen.findByRole("menuitem", { name: "启动器首页" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "显示 / 隐藏" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "退出" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "退出" }));
    expect(bridge.command).toHaveBeenCalledWith("tray_menu_action", { id: "quit" });
  });

  it("Esc 调用收起命令", async () => {
    render(<TrayMenu />);
    await screen.findByRole("menuitem", { name: "启动器首页" });
    fireEvent.keyDown(window, { key: "Escape" });
    expect(bridge.command).toHaveBeenCalledWith("tray_menu_hide");
  });
});
