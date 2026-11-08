import { StrictMode, useEffect, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { FluentProvider } from "@fluentui/react-components";
import { App } from "./App";
import { ConnectionWindow } from "./ConnectionWindow";
import { TrayMenu } from "./TrayMenu";
import { launcherDarkTheme, launcherLightTheme } from "./theme";

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
function FluentShell({ children }: { children: ReactNode }) {
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
  return (
    <FluentProvider
      theme={light ? launcherLightTheme : launcherDarkTheme}
      style={{ backgroundColor: "transparent" }}
    >
      {children}
    </FluentProvider>
  );
}

function Root() {
  return (
    <FluentShell>
      <App />
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
if (isTrayMenuWindow) {
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
