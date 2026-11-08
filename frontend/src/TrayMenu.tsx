// 托盘右键自绘菜单（?view=tray）：整个窗口即一枚 Fluent 风格悬浮菜单，
// 由 Rust 侧预建的无边框透明窗口承载（见 src/tray_menu.rs）。
// 菜单项动作经 tray_menu_action 命令回传，复用原生菜单同一套业务逻辑。
import { useEffect, useState } from "react";
import { command, getContext } from "./bridge";
import { translator, type Key } from "./i18n";
import type { LauncherSettings } from "./types";
import styles from "./trayMenu.module.css";

const items: { id: string; key: Key }[] = [
  { id: "home", key: "home" },
  { id: "toggle", key: "toggle_home" },
];

export function TrayMenu() {
  const [locale, setLocale] = useState("zh-CN");
  const [motion, setMotion] = useState(true);
  const t = translator(locale);

  useEffect(() => {
    // 无边框透明窗口：清除 index.html 的首帧底色，让菜单圆角外全透明。
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    let disposed = false;
    getContext()
      .then((c) => {
        if (!disposed) {
          setLocale(c.locale);
          document.documentElement.lang = c.locale;
        }
      })
      .catch(() => {});
    // 减弱动态效果时跳过入场动画；读取失败保持动画（非 Tauri 预览等）。
    command<LauncherSettings>("settings_get")
      .then((s) => {
        if (!disposed) setMotion(!s.reducedMotion);
      })
      .catch(() => {});
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    // 右键弹窗本身不应再弹系统默认菜单；Esc 收起弹窗。
    const block = (event: MouseEvent) => event.preventDefault();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void command("tray_menu_hide").catch(() => {});
    };
    document.addEventListener("contextmenu", block);
    window.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("contextmenu", block);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const run = (id: string) => {
    void command("tray_menu_action", { id }).catch(() => {});
  };

  return (
    <nav
      className={styles.menu}
      role="menu"
      aria-label="AzurPilot"
      data-motion={motion ? "on" : "off"}
      style={motion ? undefined : { animation: "none" }}
    >
      {items.map((item) => (
        <button
          key={item.id}
          type="button"
          role="menuitem"
          className={styles.item}
          onClick={() => run(item.id)}
        >
          {t(item.key)}
        </button>
      ))}
      <div className={styles.separator} role="separator" />
      <button
        type="button"
        role="menuitem"
        className={styles.item}
        onClick={() => run("quit")}
      >
        {t("quit")}
      </button>
    </nav>
  );
}
