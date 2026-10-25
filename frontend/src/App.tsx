// Content rank: launch action > service/task status > version and logs.
// WinUI 3 / Fluent 2 look via official @fluentui/react-components; layout
// tokens (Mica backdrop, hero ramp, spacing) stay in launcher.module.css.
// One primary action per state, all state logic identical to before.
import { useEffect, useMemo, useRef, useState, type ComponentType } from "react";
import { Button, Spinner } from "@fluentui/react-components";
import {
  ArrowClockwise16Filled,
  ArrowClockwise16Regular,
  ArrowDownload16Regular,
  ArrowLeft16Regular,
  DismissRegular,
  ErrorCircle16Regular,
  FullScreenMaximizeRegular,
  Info16Regular,
  Open16Filled,
  Play16Filled,
  Settings16Regular,
  Stop16Regular,
  SubtractRegular,
  Wrench16Filled,
  Wrench16Regular,
} from "@fluentui/react-icons";
import { command, getContext, native, subscribe } from "./bridge";
import { translator } from "./i18n";
import { acceptSnapshot, busy, type Context, type LauncherSettings, type Snapshot } from "./types";
import { Confirm } from "./Confirm";
import { ErrorDialog } from "./ErrorDialog";
import { SettingsDialog } from "./SettingsDialog";
import background from "../../bg/bg.mp4";
import backgroundPoster from "../../bg/loading.webp";
import styles from "./launcher.module.css";

const mainIcons: Record<string, ComponentType> = {
  launcher_start: Play16Filled,
  launcher_deploy: ArrowDownload16Regular,
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
  const [bridgeError, setBridgeError] = useState("");
  const [armCancel, setArmCancel] = useState(false);
  const [errorDialogOpen, setErrorDialogOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const video = useRef<HTMLVideoElement>(null);
  const [localeOverride, setLocaleOverride] = useState<string | null>(null);
  const [reducedMotion, setReducedMotion] = useState(false);
  const [audioEnabled, setAudioEnabled] = useState(false);
  const t = useMemo(
    () => translator(localeOverride ?? context.locale),
    [localeOverride, context.locale],
  );
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
    // 语言偏好（设置里选定的 UI 语言）覆盖系统语言；同时恢复动效与音频偏好。
    command<LauncherSettings>("settings_get")
      .then((s) => {
        if (disposed) return;
        if (s?.language) setLocaleOverride(s.language);
        setReducedMotion(!!s?.reducedMotion);
        setAudioEnabled(!!s?.audioEnabled);
      })
      .catch(() => {
        /* 设置读取失败时沿用系统语言与默认偏好 */
      });
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
  // 背景视频受三层控制：页面隐藏、系统或设置任一开启“减弱动态效果”。
  // 视频持续可见播放会让 WebView2 持有显示器唤醒锁，导致系统无法息屏。
  // 静音状态由设置的音频开关驱动（默认静音；React 的 muted 属性对
  // video 元素不可靠，必须直接写 property）。
  useEffect(() => {
    const motion = matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => {
      const element = video.current;
      if (!element) return;
      element.muted = !audioEnabled;
      if (document.hidden || motion.matches || reducedMotion) element.pause();
      else void element.play().catch(() => {});
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    motion.addEventListener("change", sync);
    return () => {
      document.removeEventListener("visibilitychange", sync);
      motion.removeEventListener("change", sync);
    };
  }, [reducedMotion, audioEnabled]);
  // CSS 动画（步骤文字淡入、进度填充过渡）随设置一并停用。
  useEffect(() => {
    document.documentElement.classList.toggle("reduced-motion", reducedMotion);
  }, [reducedMotion]);
  // 主页屏蔽浏览器默认交互：右键菜单、拖拽与文本选择（错误弹窗的日志
  // 块标注 data-selectable 保持可复制），以及 F5 / Ctrl+R 触发的刷新。
  useEffect(() => {
    const blockUnlessSelectable = (event: Event) => {
      const target = event.target;
      if (
        target instanceof Element &&
        target.closest("[data-selectable]")
      )
        return;
      event.preventDefault();
    };
    const blockReload = (event: KeyboardEvent) => {
      if (
        event.key === "F5" ||
        (event.ctrlKey && event.key.toLowerCase() === "r")
      )
        event.preventDefault();
    };
    document.addEventListener("contextmenu", blockUnlessSelectable);
    document.addEventListener("selectstart", blockUnlessSelectable);
    document.addEventListener("dragstart", blockUnlessSelectable);
    document.addEventListener("keydown", blockReload);
    return () => {
      document.removeEventListener("contextmenu", blockUnlessSelectable);
      document.removeEventListener("selectstart", blockUnlessSelectable);
      document.removeEventListener("dragstart", blockUnlessSelectable);
      document.removeEventListener("keydown", blockReload);
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
    try {
      await command<string | boolean>(name);
      setDialog(null);
    } catch {
      setDialog(null);
    } finally {
      setPending(false);
    }
  };
  const phase = state?.phase ?? "checking";
  const active = !!state && busy(state);
  const error = connection ? state?.connection_error : state?.error;
  const shownError = error || bridgeError;
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
            : state?.deployment_required
              ? "launcher_deploy"
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
            : state?.deployment_required
              ? "deploy"
              : phase === "error"
              ? "retry_start"
              : "start";
  const canAct =
    connection ||
    state?.can_update ||
    (state?.backend_running && phase === "running") ||
    state?.can_start ||
    state?.can_deploy ||
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
    !state?.update_version &&
    !state?.backend_running;
  // 任务进行中同样收起面板：右下角大按钮以转圈与轮播文字表达进度。
  const launching = active && !connection && !error;
  // 两段式取消：第一次点击进入待确认态并保持，再次点击才取消；
  // 任务结束（launching 变 false）时自动复位。
  useEffect(() => {
    if (!launching) setArmCancel(false);
  }, [launching]);
  // 出现新错误时自动弹出错误报告（PCL 风格）；关闭后可经“查看错误详情”重开。
  useEffect(() => {
    if (shownError) setErrorDialogOpen(true);
  }, [shownError]);
  // 弹窗标题优先按分类错误码（deploy ErrorCodes 10-13 / gui EXIT_* 70-76），
  // 无码时回退按操作类型，再回退通用文案。
  const errorCodeTitles: Record<number, Parameters<typeof t>[0]> = {
    10: "err_command",
    11: "err_git_fetch",
    12: "err_git_merge",
    13: "err_deps",
    70: "err_startup",
    71: "err_backend",
    72: "err_deps",
    73: "err_frontend",
    74: "err_backend",
    75: "err_backend",
    76: "err_backend",
  };
  const errorTitle = connection
    ? t("connection_title")
    : (state?.error_code != null && errorCodeTitles[state.error_code]) ||
      (!state || !state.operation
        ? t("error")
        : state.operation === "repair"
          ? t("repair_failed")
          : state.operation === "deploy"
            ? t("deploy_failed")
            : state.operation === "update"
              ? t("update_failed")
              : state.operation === "check"
                ? t("check_failed")
                : t("launch_failed"));
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
        poster={backgroundPoster}
        autoPlay
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
            <span className={styles.brandVersion}>{context.version}</span>
          </div>
          <div className={styles.windowControls}>
            <Button
              appearance="transparent"
                className={styles.captionButton}
                aria-label={t("settings")}
                title={t("settings")}
                icon={<Settings16Regular fontSize={16} />}
                onClick={() => setSettingsOpen(true)}
              />
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
              <Button
                appearance="transparent"
                size="small"
                icon={<ArrowDownload16Regular />}
                onClick={() => void act("download_today_launcher_log")}
                disabled={pending}
              >
                {t("export_log")}
              </Button>
            </div>
            <div className={styles.launchStack}>
              {launching && (
                <span className={styles.launchHint} aria-hidden="true">
                  <Info16Regular fontSize={14} />
                  {t("click_cancel_hint")}
                </span>
              )}
              <Button
              appearance="primary"
              size="large"
              className={`${styles.primary} ${launching ? styles.launchingButton : ""}`}
              title={!launching && state?.deployment_required ? t("deploy_hint") : undefined}
              onClick={launching ? handleLaunchClick : handleMain}
              disabled={
                launching ? undefined : pending || !canAct || !!bridgeError
              }
              icon={
                launching ? (
                  armCancel ? (
                    <DismissRegular />
                  ) : (
                    <Spinner size="small" />
                  )
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
          </div>
        ) : (
          <div className={styles.bottomStack}>
          <section className={styles.panel} aria-label={t("status")}>
            {state?.update_version && (
              <p className={styles.taskDetail}>
                {t("update_hint", { version: state.update_version })}
              </p>
            )}
            {!active && !error && !state?.update_version && (
              <p className={styles.taskDetail}>
                {state?.message ||
                  t(state?.backend_running ? "repair_disabled" : state?.deployment_required ? "deploy_hint" : "start_hint")}
              </p>
            )}
            {shownError && state?.message && (
              <p className={styles.taskDetail}>{state.message}</p>
            )}
          </section>
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
              <div className={styles.launchStack}>
                {shownError && (
                  <button
                    type="button"
                    className={styles.errorSummary}
                    onClick={() => setErrorDialogOpen(true)}
                  >
                    <ErrorCircle16Regular aria-hidden="true" />
                    <span>{t("details")}</span>
                  </button>
                )}
                <Button
                  appearance="primary"
                  size="large"
                  className={styles.primary}
                  onClick={handleMain}
                  disabled={pending || !canAct || !!bridgeError}
                  icon={
                    active ? (
                      reducedMotion ? (
                        <ArrowClockwise16Regular />
                      ) : (
                        <Spinner size="extra-small" />
                      )
                    ) : MainIcon ? (
                      <MainIcon />
                    ) : undefined
                  }
                >
                  {active ? t(phase) : t(mainLabel)}
                </Button>
              </div>
            </div>
          </div>
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
      {shownError && errorDialogOpen && (
        <ErrorDialog
          title={errorTitle}
          log={shownError}
          hint={t("need_log_help")}
          downloadLabel={t("download_error_log")}
          packingLabel={t("packing_log")}
          closeLabel={t("close")}
          onDownload={async () => {
            try {
              const target = await command<string | null>("prompt_log_archive_path");
              if (!target) return false;
              await command("download_log_archive", { target });
              return true;
            } catch {
              return false;
            }
          }}
          onClose={() => setErrorDialogOpen(false)}
        />
      )}
      {settingsOpen && (
        <SettingsDialog
          title={t("settings")}
          sourceLabels={{ download: t("settings_download_source"), python: t("settings_python_source"), pypi: t("settings_pypi_source"), hint: t("settings_sources_hint"), auto: t("source_auto"), cnb: t("source_cnb"), github: t("source_github"), nju: t("source_nju"), ustc: t("source_ustc"), npmmirror: t("source_npmmirror"), aliyun: t("source_aliyun"), astral: t("source_astral"), pypi_name: t("source_pypi"), tsinghua: t("source_tsinghua"), tencent: t("source_tencent"), huawei: t("source_huawei") }}
          platform={context.platform}
          languageLabel={t("settings_language")}
          autoLanguageLabel={t("lang_auto")}
          updateChannelLabel={t("settings_update_channel")}
          channelStableLabel={t("channel_stable")}
          channelCanaryLabel={t("channel_canary")}
          updateSourceLabel={t("settings_update_source")}
          updateSourceLabels={{
            auto: t("update_source_auto"),
            official: t("update_source_official"),
            github: t("update_source_github"),
            cloudflare: t("update_source_cloudflare"),
          }}
          launchOnStartupLabel={t("settings_launch_on_startup")}
          autoStartServiceLabel={t("settings_auto_start_service")}
          keepWebviewOnTrayLabel={t("settings_keep_webview_tray")}
          reducedMotionLabel={t("settings_reduced_motion")}
          preventSleepLabel={t("settings_prevent_sleep")}
          audioEnabledLabel={t("settings_audio")}
          closeLabel={t("close")}
          unsupportedLabel={t("not_supported")}
          onLanguageChange={(locale) => {
            setLocaleOverride(locale);
            document.documentElement.lang = locale ?? context.locale;
          }}
          onReducedMotionChange={setReducedMotion}
          onAudioChange={setAudioEnabled}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}

// 任务进行中：按钮内仅显示当前步骤名（progress.title），无百分比与
// uv/git 具体输出；取消阶段固定显示“正在取消”，避免残留旧步骤标题；
// 步骤切换时由 launchText 动画呈现淡入效果。
function LaunchActivity({
  t,
  state,
}: {
  t: ReturnType<typeof translator>;
  state: Snapshot | null;
}) {
  const text =
    state?.phase === "cancelling"
      ? t("cancelling")
      : state?.progress?.title ?? (state ? t(state.phase) : "");
  if (!text) return null;
  return (
    <span className={styles.launchText} key={text}>
      {text}
    </span>
  );
}
