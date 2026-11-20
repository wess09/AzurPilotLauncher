// 托盘右键自绘菜单（?view=tray）：整个窗口即一枚 Fluent 风格悬浮菜单，
// 由 Rust 侧预建的无边框透明窗口承载（见 src/tray_menu.rs）。
// 菜单项动作经 tray_menu_action 命令回传，复用原生菜单同一套业务逻辑。
import { useEffect, useState, type ComponentType } from "react";
import {
  Eye16Regular,
  Home16Regular,
  Play16Filled,
  PlayMultiple16Filled,
  Power20Regular,
  Stop16Regular,
} from "@fluentui/react-icons";
import { command, getContext } from "./bridge";
import { translator, type Key } from "./i18n";
import type { LauncherSettings } from "./types";
import styles from "./trayMenu.module.css";

const items: { id: string; key: Key; icon: ComponentType }[] = [
  { id: "home", key: "home", icon: Home16Regular },
  { id: "toggle", key: "toggle_home", icon: Eye16Regular },
];

interface InstanceInfo {
  name: string;
  status: string;
}

export function TrayMenu() {
  const [locale, setLocale] = useState("zh-CN");
  const [motion, setMotion] = useState(true);
  const [instances, setInstances] = useState<InstanceInfo[]>([]);
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
    // 每次弹窗获得焦点时刷新实例状态：弹出前 Rust 已按实例数定好窗口尺寸。
    const refresh = () => {
      command<InstanceInfo[]>("tray_instances")
        .then((list) => {
          if (!disposed) setInstances(list);
        })
        .catch(() => {
          if (!disposed) setInstances([]);
        });
    };
    refresh();
    window.addEventListener("focus", refresh);
    return () => {
      disposed = true;
      window.removeEventListener("focus", refresh);
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
      {items.map((item) => {
        const Icon = item.icon;
        return (
          <button
            key={item.id}
            type="button"
            role="menuitem"
            className={styles.item}
            onClick={() => run(item.id)}
          >
            <span className={styles.itemIcon} aria-hidden="true">
              <Icon />
            </span>
            <span>{t(item.key)}</span>
          </button>
        );
      })}
      {instances.length > 0 && (
        <>
          <div className={styles.separator} role="separator" />
          {instances.map((instance) => {
            const running = instance.status === "running";
            return (
              <button
                key={instance.name}
                type="button"
                role="menuitem"
                className={styles.item}
                title={instance.name}
                onClick={() => run(`scheduler:${running ? "stop" : "start"}:${instance.name}`)}
              >
                <span className={styles.itemIcon} aria-hidden="true" data-running={running}>
                  {running ? <Play16Filled /> : <Stop16Regular />}
                </span>
                <span className={styles.instanceName}>{instance.name}</span>
                <span className={styles.status} data-running={running}>
                  {t(running ? "scheduler_running" : "scheduler_stopped")}
                </span>
              </button>
            );
          })}
          <div className={styles.separator} role="separator" />
          <button
            type="button"
            role="menuitem"
            className={styles.item}
            onClick={() => run("scheduler:start-all")}
          >
            <span className={styles.itemIcon} aria-hidden="true">
              <PlayMultiple16Filled />
            </span>
            <span>{t("scheduler_start_all")}</span>
          </button>
          <button
            type="button"
            role="menuitem"
            className={styles.item}
            onClick={() => run("scheduler:stop-all")}
          >
            <span className={styles.itemIcon} aria-hidden="true">
              <Stop16Regular />
            </span>
            <span>{t("scheduler_stop_all")}</span>
          </button>
        </>
      )}
      <div className={styles.separator} role="separator" />
      <button
        type="button"
        role="menuitem"
        className={styles.item}
        onClick={() => run("quit")}
      >
        <span className={styles.itemIcon} aria-hidden="true">
          <Power20Regular />
        </span>
        <span>{t("quit")}</span>
      </button>
    </nav>
  );
}
