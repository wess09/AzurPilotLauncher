import { Button, FluentProvider } from "@fluentui/react-components";
import {
  DismissCircle24Filled,
  DismissRegular,
  ArrowDownload24Regular,
} from "@fluentui/react-icons";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { translator } from "./i18n";
import type { Snapshot } from "./types";
import { launcherDarkTheme, launcherLightTheme } from "./theme";
import styles from "./launcher.module.css";

// 后端崩溃报告独立窗口（?window=crash）：WinUI 3 ContentDialog 布局——
// 标题（错误图标 + 标题）+ 正文一句话人话原因 + 右下角操作栏（关闭 / 下载日志）。
// 由 src/native_dialog.rs 以独立 Tauri WebviewWindow 打开（label=crash），
// 仅授权 launcher_context/launcher_snapshot/download_today_gui_log/
// window_close/window_start_dragging（见 capabilities/crash-dialog.json）。
export function CrashWindow() {
  const [light, setLight] = useState(
    matchMedia("(prefers-color-scheme: light)").matches,
  );
  const [locale, setLocale] = useState<string | null>(null);
  const [summary, setSummary] = useState("");
  const [hint, setHint] = useState("");
  const windowRef = useRef<HTMLDivElement>(null);

  // 内容渲染后按实际高度贴合窗口；摘要为空时也要等首帧拿到稳定高度。
  useLayoutEffect(() => {
    const el = windowRef.current;
    if (!el) return;
    const fit = () => {
      const rect = el.getBoundingClientRect();
      void invoke("window_fit_content", {
        width: Math.ceil(rect.width),
        height: Math.ceil(rect.height),
      }).catch(() => {});
    };
    fit();
    // 字体加载完成后文案可能换行，重新测一次。
    document.fonts?.ready.then(fit).catch(() => {});
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, [summary, hint, locale]);
  useEffect(() => {
    const query = matchMedia("(prefers-color-scheme: light)");
    const sync = (e: MediaQueryListEvent) => setLight(e.matches);
    query.addEventListener("change", sync);
    return () => query.removeEventListener("change", sync);
  }, []);
  useEffect(() => {
    // 无边框透明窗口：清除首帧底色，让窗口圆角外全透明。
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
  }, []);
  useEffect(() => {
    let disposed = false;
    invoke<{ locale: string }>("launcher_context")
      .then((c) => {
        if (!disposed && c?.locale) setLocale(c.locale);
      })
      .catch(() => {});
    invoke<Snapshot>("launcher_snapshot")
      .then((s) => {
        if (disposed) return;
        setSummary(s.crash?.summary ?? s.error ?? "");
      })
      .catch(() => {});
    return () => {
      disposed = true;
    };
  }, []);
  const t = useMemo(() => translator(locale ?? "zh-CN"), [locale]);
  const close = () => {
    void invoke("window_close").catch(() => {});
  };
  const downloadLog = async () => {
    try {
      await invoke("download_today_gui_log");
    } catch {
      setHint(t("action_failed"));
    }
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const startDrag = (e: React.PointerEvent) => {
    if (e.button === 0 && e.currentTarget === e.target) {
      void invoke("window_start_dragging").catch(() => {});
    }
  };
  return (
    <FluentProvider
      theme={light ? launcherLightTheme : launcherDarkTheme}
      style={{ height: "100vh", backgroundColor: "transparent" }}
    >
      <div className={styles.crashWindow} ref={windowRef}>
        <div
          className={styles.connectionDragRegion}
          onPointerDown={startDrag}
        />
        <Button
          appearance="subtle"
          aria-label="Dismiss"
          icon={<DismissRegular />}
          className={styles.messageBoxCloseBtn}
          onClick={close}
        />
        <div className={styles.messageBoxContent}>
          <div className={styles.messageBoxHeader}>
            <div className={styles.messageBoxTitleGroup}>
              <span className={styles.messageBoxErrorIcon} aria-hidden="true">
                <DismissCircle24Filled />
              </span>
              <h2 className={styles.messageBoxTitle} id="crash-title">
                {t("crash_title")}
              </h2>
            </div>
          </div>
          <div className={styles.messageBoxInnerContent}>
            {summary && (
              <p className={styles.messageBoxBodyText}>{summary}</p>
            )}
            {hint && (
              <p className={styles.connectionHint} role="alert">
                {hint}
              </p>
            )}
          </div>
        </div>
        <div className={`${styles.messageBoxFooter} ${styles.messageBoxFooterEqual}`}>
          <Button className={styles.messageBoxBtn} onClick={close}>
            {t("crash_close")}
          </Button>
          <Button
            appearance="primary"
            className={styles.messageBoxPrimaryBtn}
            onClick={() => void downloadLog()}
            autoFocus
          >
            {t("crash_download_log")}
          </Button>
        </div>
      </div>
    </FluentProvider>
  );
}
