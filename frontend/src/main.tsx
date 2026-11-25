import { StrictMode, useEffect, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { FluentProvider } from "@fluentui/react-components";
import { App } from "./App";
import { ConnectionWindow } from "./ConnectionWindow";
import { CrashWindow } from "./CrashWindow";
import { TrayMenu } from "./TrayMenu";
import { command, SETTINGS_CHANGED_EVENT } from "./bridge";
import type { LauncherSettings } from "./types";
import { defaultAccent, makeTheme } from "./theme";
// 全局 NS2 彩虹焦点环：覆盖 Fluent 组件默认的深色焦点框。
import "./focusRing.css";

// 所有窗口统一禁止右键系统菜单。主页、连接错误弹窗、崩溃报告弹窗、
// 托盘菜单都从本入口加载；捕获阶段拦截，避免子元素 stopPropagation 漏出。
document.addEventListener("contextmenu", (event) => event.preventDefault(), true);

// 开发预览可用 ?theme=dark|light 强制主题，生产始终跟随系统。
const forced =
  import.meta.env.DEV && !native()
    ? new URLSearchParams(location.search).get("theme")
    : null;

// 与 bridge.ts 的预览模式保持一致：仅开发服务器内视为非 Tauri 环境。
function native() {
  return "__TAURI_INTERNALS__" in window;
}

// 主题外壳：跟随系统深浅色，主页与托盘弹窗菜单共用。
function FluentShell({ children }: { children: ReactNode | ((light: boolean) => ReactNode) }) {
  const [light, setLight] = useState(
    forced ? forced === "light" : matchMedia("(prefers-color-scheme: light)").matches,
  );
  useEffect(() => {
    if (forced) return;
    const query = matchMedia("(prefers-color-scheme: light)");
    const sync = (e: MediaQueryListEvent) => setLight(e.matches);
    query.addEventListener("change", sync);
    return () => query.removeEventListener("change", sync);
  }, []);
  const [accent, setAccent] = useState<string | null>(null);
  useEffect(() => {
    const refresh = () => {
      void command<LauncherSettings>("settings_get")
        .then((settings) => setAccent(settings?.accentColor ?? null))
        .catch(() => {});
    };
    refresh();
    window.addEventListener(SETTINGS_CHANGED_EVENT, refresh);
    return () => window.removeEventListener(SETTINGS_CHANGED_EVENT, refresh);
  }, []);
  const theme = makeTheme(accent, light);
  useEffect(() => {
    document.documentElement.style.setProperty(
      "--accent",
      theme.colorBrandForeground1,
    );
  }, [theme.colorBrandForeground1]);
  return (
    <FluentProvider
      theme={theme}
      style={{ backgroundColor: "transparent" }}
    >
      {typeof children === "function" ? children(light) : children}
    </FluentProvider>
  );
}

function Root() {
  return (
    <FluentShell>
      {(light) => <App defaultAccentColor={defaultAccent(light)} />}
    </FluentShell>
  );
}
// 独立连接错误弹窗窗口（?window=connection 或 #/connection-dialog）加载
// 专用组件树：整个窗口即报错弹窗，不渲染主页的视频背景与状态逻辑。
const isConnectionDialogWindow =
  new URLSearchParams(location.search).get("window") === "connection" ||
  location.hash.startsWith("#/connection-dialog");
// 托盘右键自绘菜单窗口（?view=tray）：整个窗口即一枚悬浮菜单，
// 不渲染主页的视频背景与状态逻辑。
const isTrayMenuWindow =
  new URLSearchParams(location.search).get("view") === "tray";
// 后端崩溃报告独立窗口（?window=crash）：人话摘要 + 可展开详情 + 下载日志。
const isCrashWindow =
  new URLSearchParams(location.search).get("window") === "crash";
if (isCrashWindow) {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <FluentShell>
        <CrashWindow />
      </FluentShell>
    </StrictMode>,
  );
} else if (isTrayMenuWindow) {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <FluentShell>
        <TrayMenu />
      </FluentShell>
    </StrictMode>,
  );
} else if (isConnectionDialogWindow) {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <ConnectionWindow />
    </StrictMode>,
  );
} else {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <Root />
    </StrictMode>,
  );
}
