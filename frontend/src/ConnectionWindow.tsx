import { Button, FluentProvider } from "@fluentui/react-components";
import { DismissCircle24Filled, DismissRegular } from "@fluentui/react-icons";
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { translator } from "./i18n";
import type { Snapshot } from "./types";
import { launcherDarkTheme, launcherLightTheme } from "./theme";
import styles from "./launcher.module.css";

// 连接错误独立窗口的内容：整个窗口即 WinUI 3 报错弹窗——
// 顶部内容区（红色错误图标 + 标题 + 原因卡片）+ 底部灰底等宽操作栏。
// 由 src/native_dialog.rs 以独立 Tauri WebviewWindow 打开（label=connection），
// 仅授权 launcher_context/launcher_snapshot/retry_backend_connection/
// window_exit_application 四个命令（见 capabilities/connection-dialog.json）。
export function ConnectionWindow() {
  const [light, setLight] = useState(
    matchMedia("(prefers-color-scheme: light)").matches,
  );
  const [locale, setLocale] = useState<string | null>(null);
  const [detail, setDetail] = useState("");
  const [hint, setHint] = useState("");
  const [retrying, setRetrying] = useState(false);
  useEffect(() => {
    const query = matchMedia("(prefers-color-scheme: light)");
    const sync = (e: MediaQueryListEvent) => setLight(e.matches);
    query.addEventListener("change", sync);
    return () => query.removeEventListener("change", sync);
  }, []);
  useEffect(() => {
    // 无边框透明窗口：清除 index.html 的首帧深色底色，让窗口圆角外全透明。
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
        if (!disposed) setDetail(s.connection_error ?? "");
      })
      .catch(() => {});
    return () => {
      disposed = true;
    };
  }, []);
  const t = useMemo(() => translator(locale ?? "zh-CN"), [locale]);
  const retry = async () => {
    if (retrying) return;
    setRetrying(true);
    try {
      // 成功时 Rust 侧已把 main 导航回 WebUI 并关闭本窗口。
      const ok = await invoke<boolean>("retry_backend_connection");
      if (!ok) setHint(t("connection_retry_failed"));
    } catch {
      setHint(t("connection_retry_failed"));
    } finally {
      setRetrying(false);
    }
  };
  const exit = () => {
    void invoke("window_exit_application").catch(() => {});
  };
  const close = () => {
    void invoke("window_close").catch(() => {});
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const startDrag = (e: React.PointerEvent) => {
    // 只响应标题区空白处的左键拖动；按钮点击不受影响。
    if (e.button === 0 && e.currentTarget === e.target) {
      void invoke("window_start_dragging").catch(() => {});
    }
  };
  return (
    <FluentProvider
      theme={light ? launcherLightTheme : launcherDarkTheme}
      style={{ height: "100vh", backgroundColor: "transparent" }}
    >
      <div className={styles.connectionWindow}>
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
              <h2 className={styles.messageBoxTitle} id="connection-title">
                {t("connection_title")}
              </h2>
            </div>
          </div>
          <div className={styles.messageBoxInnerContent}>
            {detail && (
              <pre className={styles.messageBoxLogCard} data-selectable>
                {detail}
              </pre>
            )}
            {hint && (
              <p className={styles.connectionHint} role="alert">
                {hint}
              </p>
            )}
          </div>
        </div>
        <div className={`${styles.messageBoxFooter} ${styles.messageBoxFooterEqual}`}>
          <Button
            className={styles.messageBoxBtn}
            onClick={exit}
            disabled={retrying}
          >
            {t("quit")}
          </Button>
          <Button
            appearance="primary"
            className={styles.messageBoxPrimaryBtn}
            onClick={() => void retry()}
            disabled={retrying}
            autoFocus
          >
            {t("retry_connection")}
          </Button>
        </div>
      </div>
    </FluentProvider>
  );
}
