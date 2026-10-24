// Content rank: launch action > service/task status > version and logs.
// WinUI 3 / Fluent 2 look via official @fluentui/react-components; layout
// tokens (Mica backdrop, hero ramp, spacing) stay in launcher.module.css.
// One primary action per state, all state logic identical to before.
import { useEffect, useMemo, useRef, useState, type ComponentType } from "react";
import { Button, MessageBar, Spinner } from "@fluentui/react-components";
import {
  ArrowClockwise16Filled,
  ArrowClockwise16Regular,
  ArrowDownload16Regular,
  ArrowLeft16Regular,
  ChevronDown16Regular,
  DismissRegular,
  ErrorCircle16Regular,
  FullScreenMaximizeRegular,
  Open16Filled,
  Play16Filled,
  Stop16Regular,
  SubtractRegular,
  Wrench16Filled,
  Wrench16Regular,
} from "@fluentui/react-icons";
import { command, getContext, native, subscribe } from "./bridge";
import { translator } from "./i18n";
import { acceptSnapshot, busy, type Context, type Snapshot } from "./types";
import { Confirm } from "./Confirm";
import background from "../../bg/bg.mp4";
import styles from "./launcher.module.css";

const mainIcons: Record<string, ComponentType> = {
  launcher_start: Play16Filled,
  launcher_open_webui: Open16Filled,
  launcher_install_update: ArrowClockwise16Filled,
  launcher_check_update: ArrowClockwise16Filled,
  launcher_repair: Wrench16Filled,
  retry_backend_connection: ArrowClockwise16Filled,
};

export function App() {
  const [context, setContext] = useState<Context>({
    locale: "zh-CN",
    version: "",
    platform: "windows",
  });
  const [state, setState] = useState<Snapshot | null>(null);
  const [dialog, setDialog] = useState<"repair" | "close" | null>(null);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState("");
  const [noticeFailed, setNoticeFailed] = useState(false);
  const [bridgeError, setBridgeError] = useState("");
  const [armCancel, setArmCancel] = useState(false);
  const video = useRef<HTMLVideoElement>(null);
  const t = useMemo(() => translator(context.locale), [context.locale]);
  const connection = location.hash.startsWith("#/connection-error");
  useEffect(() => {
    let disposed = false;
    let cleanup: (() => void) | undefined;
    getContext()
      .then((c) => {
        if (!disposed) {
          setContext(c);
          document.documentElement.lang = c.locale;
        }
      })
      .catch((e) => setBridgeError(String(e)));
    subscribe(
      (s) => {
        if (!disposed) {
          setState((current) => acceptSnapshot(current, s));
          setBridgeError("");
        }
      },
      () => {
        if (!disposed) setDialog("close");
      },
    )
      .then((fn) => {
        if (disposed) fn();
        else cleanup = fn;
      })
      .catch((e) => {
        if (!disposed) setBridgeError(String(e));
      });
    return () => {
      disposed = true;
      cleanup?.();
    };
  }, []);
  useEffect(() => {
    const motion = matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => {
      if (document.hidden || motion.matches) video.current?.pause();
      else void video.current?.play().catch(() => {});
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    motion.addEventListener("change", sync);
    return () => {
      document.removeEventListener("visibilitychange", sync);
      motion.removeEventListener("change", sync);
    };
  }, []);
  useEffect(() => {
    if (!connection || !state?.backend_running) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    const retry = async () => {
      try {
        if (await command<boolean>("retry_backend_connection")) return;
      } catch {
        /* Manual retry remains available. */
      }
      if (!stopped) timer = setTimeout(() => void retry(), 1000);
    };
    timer = setTimeout(() => void retry(), 1000);
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [connection, state?.backend_running, state?.task_id]);
  const act = async (name: string) => {
    if (pending) return;
    setPending(true);
    setNotice("");
    try {
      const result = await command<string | boolean>(name);
      if (name.includes("download_today_") && result) {
        setNotice(t("log_saved"));
        setNoticeFailed(false);
      }
      setDialog(null);
    } catch (error) {
      setNotice(`${t("action_failed")} ${String(error)}`);
      setNoticeFailed(true);
    } finally {
      setPending(false);
    }
  };
  const phase = state?.phase ?? "checking";
  const active = !!state && busy(state);
  const error = connection ? state?.connection_error : state?.error;
  const mainAction = connection
    ? "retry_backend_connection"
    : state?.can_update
      ? "launcher_install_update"
      : phase === "error" && state?.operation === "check"
        ? "launcher_check_update"
        : state?.backend_running
          ? "launcher_open_webui"
          : phase === "error" && state?.operation === "repair"
            ? "launcher_repair"
            : "launcher_start";
  const mainLabel = connection
    ? "retry_connection"
    : state?.can_update
      ? "update"
      : phase === "error" && state?.operation === "check"
        ? "retry_check"
        : state?.backend_running
          ? "open_webui"
          : phase === "error" && state?.operation === "repair"
            ? "retry_repair"
            : phase === "error"
              ? "retry_start"
              : "start";
  const canAct =
    connection ||
    state?.can_update ||
    (state?.backend_running && phase === "running") ||
    state?.can_start ||
    (phase === "error" && state?.can_check) ||
    (mainAction === "launcher_repair" && state?.can_repair);
  const handleMain = () =>
    mainAction === "launcher_repair"
      ? setDialog("repair")
      : void act(mainAction);
  const MainIcon = mainIcons[mainAction];
  // 空闲态去掉面板卡片，只在右下角保留主操作按钮。
  // 后端运行中不视为空闲：面板承载停止服务与禁用态修复入口。
  const idle =
    !connection &&
    !active &&
    !error &&
    !bridgeError &&
    !notice &&
    !state?.update_version &&
    !state?.backend_running;
  // 任务进行中同样收起面板：右下角大按钮以转圈与轮播文字表达进度。
  const launching = active && !connection && !error;
  // 两段式取消：第一次点击进入待确认态，3 秒内再按一次才真正取消。
  useEffect(() => {
    if (!launching) setArmCancel(false);
  }, [launching]);
  useEffect(() => {
    if (!armCancel) return;
    const timer = setTimeout(() => setArmCancel(false), 3000);
    return () => clearTimeout(timer);
  }, [armCancel]);
  const handleLaunchClick = () => {
    if (armCancel) {
      setArmCancel(false);
      void act("launcher_cancel");
    } else {
      setArmCancel(true);
    }
  };
  return (
    <div className={styles.shell}>
      <video
        ref={video}
        className={styles.background}
        src={background}
        autoPlay
        muted
        loop
        playsInline
        aria-hidden="true"
      />
      <div className={styles.shade} />
      {!connection && (
        <header
          className={styles.titlebar}
          data-tauri-drag-region
          onPointerDown={(e) => {
            if (
              native &&
              context.platform !== "windows" &&
              e.isPrimary &&
              e.button === 0 &&
              e.target === e.currentTarget
            )
              void command("window_start_dragging").catch(() => {});
          }}
        >
          <div className={styles.brand}>
            <img className={styles.logo} src="/icon.png" alt="" />
            <span className={styles.wordmark}>
              Azur<span>Pilot</span>
            </span>
          </div>
          {context.platform !== "macos" && (
            <div className={styles.windowControls}>
              <Button
                appearance="transparent"
                className={styles.captionButton}
                aria-label={t("minimize")}
                title={t("minimize")}
                icon={<SubtractRegular fontSize={16} />}
                onClick={() => void act("window_minimize")}
              />
              <Button
                appearance="transparent"
                className={styles.captionButton}
                aria-label={t("maximize")}
                title={t("maximize")}
                icon={<FullScreenMaximizeRegular fontSize={16} />}
                onClick={() => void act("window_toggle_maximize")}
              />
              <Button
                appearance="transparent"
                className={`${styles.captionButton} ${styles.closeButton}`}
                aria-label={t("close")}
                title={t("close")}
                icon={<DismissRegular fontSize={16} />}
                onClick={() =>
                  active ? setDialog("close") : void act("window_close")
                }
              />
            </div>
          )}
        </header>
      )}
      <main className={`${styles.main} ${connection ? styles.connection : ""}`}>
        <section className={styles.hero}>
          {connection && (
            <>
              <h1>{t("connection_title")}</h1>
              <p className={styles.description}>{t("connection_body")}</p>
            </>
          )}
        </section>
        {idle || launching ? (
          <div className={styles.idleBar}>
            {launching && (
              <span className={styles.launchHint} aria-hidden="true">
                {t("click_cancel_hint")}
              </span>
            )}
            <Button
              appearance="primary"
              size="large"
              className={`${styles.primary} ${launching ? styles.launchingButton : ""}`}
              onClick={launching ? handleLaunchClick : handleMain}
              disabled={
                launching ? undefined : pending || !canAct || !!bridgeError
              }
              icon={
                launching ? (
                  <Spinner size="small" />
                ) : MainIcon ? (
                  <MainIcon />
                ) : undefined
              }
            >
              {launching && (
                <span
                  className={styles.launchFill}
                  style={{ width: `${state?.progress?.progress ?? 0}%` }}
                  aria-hidden="true"
                />
              )}
              {launching ? (
                armCancel ? (
                  <span className={styles.launchText} key="arm">
                    {t("press_again_cancel")}
                  </span>
                ) : (
                  <LaunchActivity t={t} state={state} />
                )
              ) : (
                t(mainLabel)
              )}
            </Button>
          </div>
        ) : (
          <section className={styles.panel} aria-label={t("status")}>
            <div className={styles.statusRow}>
              <span
                className={`${styles.indicator} ${phase === "error" || error ? styles.failed : ""}`}
              />
              <span className={styles.status} role="status">
                {connection ? t("connection_title") : t(phase)}
              </span>
              <span className={styles.version}>
                {t("version")} {context.version}
              </span>
            </div>
            {state?.update_version && (
              <p className={styles.taskDetail}>
                {t("update_hint", { version: state.update_version })}
              </p>
            )}
            {!active && !error && !state?.update_version && (
              <p className={styles.taskDetail}>
                {state?.message ||
                  t(state?.backend_running ? "repair_disabled" : "start_hint")}
              </p>
            )}
            {(error || bridgeError) && (
              <details className={styles.errorDetails} open>
                <summary className={styles.errorSummary}>
                  <ErrorCircle16Regular aria-hidden="true" />
                  <span>{t("details")}</span>
                  <ChevronDown16Regular className={styles.chevron} aria-hidden="true" />
                </summary>
                <pre className={styles.errorLog}>{error || bridgeError}</pre>
              </details>
            )}
            {notice && (
              <MessageBar
                className={styles.notice}
                intent={noticeFailed ? "error" : "success"}
                role="status"
              >
                {notice}
              </MessageBar>
            )}
            <div className={styles.actions}>
              <div className={styles.secondaryActions}>
                {!connection && (
                  <Button
                    appearance="transparent"
                    size="small"
                    icon={<Wrench16Regular />}
                    onClick={() => setDialog("repair")}
                    disabled={pending || !state?.can_repair}
                    title={
                      state?.backend_running ? t("repair_disabled") : t("repair")
                    }
                  >
                    {t("repair")}
                  </Button>
                )}
                {state?.can_stop && !connection && (
                  <Button
                    appearance="transparent"
                    size="small"
                    icon={<Stop16Regular />}
                    onClick={() => void act("launcher_stop")}
                    disabled={pending}
                  >
                    {t("stop")}
                  </Button>
                )}
                <Button
                  appearance="transparent"
                  size="small"
                  icon={<ArrowDownload16Regular />}
                  onClick={() => void act("download_today_launcher_log")}
                  disabled={pending}
                >
                  {t("export_log")}
                </Button>
                {connection && (
                  <Button
                    appearance="transparent"
                    size="small"
                    icon={<ArrowDownload16Regular />}
                    onClick={() => void act("download_today_gui_log")}
                    disabled={pending}
                  >
                    {t("export_gui_log")}
                  </Button>
                )}
                {(connection ||
                  (phase === "error" &&
                    state?.operation !== "check" &&
                    state?.operation !== "update")) && (
                  <Button
                    appearance="transparent"
                    size="small"
                    icon={<ArrowLeft16Regular />}
                    onClick={() =>
                      void act(
                        connection ? "launcher_show_home" : "launcher_back",
                      )
                    }
                    disabled={pending}
                  >
                    {t("back")}
                  </Button>
                )}
              </div>
              <Button
                appearance="primary"
                size="large"
                className={styles.primary}
                onClick={handleMain}
                disabled={pending || !canAct || !!bridgeError}
                icon={
                  active ? (
                    <Spinner size="extra-small" />
                  ) : MainIcon ? (
                    <MainIcon />
                  ) : undefined
                }
              >
                {active ? t(phase) : t(mainLabel)}
              </Button>
            </div>
          </section>
        )}
      </main>
      {dialog && (
        <Confirm
          title={t(dialog === "repair" ? "repair_title" : "close_title")}
          body={t(dialog === "repair" ? "repair_body" : "close_body")}
          confirm={t(dialog === "repair" ? "repair_confirm" : "exit_confirm")}
          cancel={t("cancel")}
          pending={pending}
          onCancel={() => setDialog(null)}
          onConfirm={() =>
            void act(
              dialog === "repair"
                ? "launcher_repair"
                : "launcher_cancel_and_exit",
            )
          }
        />
      )}
    </div>
  );
}

// 任务进行中：按钮内仅显示当前步骤名（progress.title），无百分比与
// uv/git 具体输出；步骤切换时由 launchText 动画呈现淡入效果。
function LaunchActivity({
  t,
  state,
}: {
  t: ReturnType<typeof translator>;
  state: Snapshot | null;
}) {
  const text = state?.progress?.title ?? (state ? t(state.phase) : "");
  if (!text) return null;
  return (
    <span className={styles.launchText} key={text}>
      {text}
    </span>
  );
}
