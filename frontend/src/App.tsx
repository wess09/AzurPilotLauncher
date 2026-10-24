// Content rank: launch action > service/task status > version and logs.
// Shared tokens: 8px spacing rhythm, 8/16px radii, 14/16/24/48px type,
// dark navy surfaces, existing blue accent. One primary action per state.
import { useEffect, useMemo, useRef, useState } from "react";
import { command, getContext, native, subscribe } from "./bridge";
import { translator } from "./i18n";
import { acceptSnapshot, busy, type Context, type Snapshot } from "./types";
import { Progress } from "./TaskProgress";
import { Confirm } from "./Confirm";
import background from "../../bg/bg.mp4";
import styles from "./launcher.module.css";

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
  const [bridgeError, setBridgeError] = useState("");
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
      if (name.includes("download_today_") && result) setNotice(t("log_saved"));
      setDialog(null);
    } catch (error) {
      setNotice(`${t("action_failed")} ${String(error)}`);
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
          <span className={styles.wordmark}>
            AZUR<span>PILOT</span>
          </span>
          {context.platform !== "macos" && (
            <div className={styles.windowControls}>
              <button
                aria-label={t("minimize")}
                title={t("minimize")}
                onClick={() => void act("window_minimize")}
              >
                −
              </button>
              <button
                aria-label={t("maximize")}
                title={t("maximize")}
                onClick={() => void act("window_toggle_maximize")}
              >
                □
              </button>
              <button
                aria-label={t("close")}
                title={t("close")}
                className={styles.close}
                onClick={() =>
                  active ? setDialog("close") : void act("window_close")
                }
              >
                ×
              </button>
            </div>
          )}
        </header>
      )}
      <main className={`${styles.main} ${connection ? styles.connection : ""}`}>
        <section className={styles.hero}>
          <p className={styles.eyebrow}>{t("tagline")}</p>
          <h1>{connection ? t("connection_title") : t("headline")}</h1>
          <p className={styles.description}>
            {connection ? t("connection_body") : t("description")}
          </p>
        </section>
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
          {state?.progress && active && (
            <>
              <p className={styles.taskDetail}>{state.progress.detail}</p>
              <Progress state={state} t={t} />
            </>
          )}
          {!active && !error && !state?.update_version && (
            <p className={styles.taskDetail}>
              {state?.message || t(state?.backend_running ? "repair_disabled" : "start_hint")}
            </p>
          )}
          {(error || bridgeError) && (
            <details className={styles.errorDetails} open>
              <summary>{t("details")}</summary>
              <pre>{error || bridgeError}</pre>
            </details>
          )}
          {notice && (
            <p className={styles.notice} role="status">
              {notice}
            </p>
          )}
          <div className={styles.actions}>
            <div className={styles.secondaryActions}>
              {!connection && (
                <button
                  onClick={() => setDialog("repair")}
                  disabled={pending || !state?.can_repair}
                  title={
                    state?.backend_running ? t("repair_disabled") : t("repair")
                  }
                >
                  ↻ <span>{t("repair")}</span>
                </button>
              )}
              {state?.can_stop && !connection && (
                <button
                  onClick={() => void act("launcher_stop")}
                  disabled={pending}
                >
                  ■ <span>{t("stop")}</span>
                </button>
              )}
              <button
                onClick={() => void act("download_today_launcher_log")}
                disabled={pending}
              >
                {t("export_log")}
              </button>
              {connection && (
                <button
                  onClick={() => void act("download_today_gui_log")}
                  disabled={pending}
                >
                  {t("export_gui_log")}
                </button>
              )}
              {(connection ||
                (phase === "error" &&
                  state?.operation !== "check" &&
                  state?.operation !== "update")) && (
                <button
                  onClick={() =>
                    void act(
                      connection ? "launcher_show_home" : "launcher_back",
                    )
                  }
                  disabled={pending}
                >
                  {t("back")}
                </button>
              )}
            </div>
            <button
              className={styles.primary}
              onClick={handleMain}
              disabled={pending || !canAct || !!bridgeError}
            >
              {active ? (
                <>
                  <span className={styles.spinner} />
                  {t(phase)}
                </>
              ) : (
                <>
                  <span aria-hidden="true">▶</span>
                  {t(mainLabel)}
                </>
              )}
            </button>
          </div>
        </section>
      </main>
      <footer className={styles.footer}>
        <span>AZURPILOT LAUNCHER</span>
        <span>{t("footer")}</span>
      </footer>
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
