import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { FluentProvider } from "@fluentui/react-components";
import { App } from "./App";
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

function Root() {
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
      <App />
    </FluentProvider>
  );
}
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
