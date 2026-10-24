import { useEffect, useRef, useState } from "react";
import { ProgressBar } from "@fluentui/react-components";
import { advance, channel, target } from "./progress";
import type { Snapshot } from "./types";
import type { translator } from "./i18n";
import styles from "./launcher.module.css";

export function Progress({
  state,
  t,
}: {
  state: Snapshot;
  t: ReturnType<typeof translator>;
}) {
  const channels = useRef({
    task: -1,
    uvTask: -1,
    main: channel(),
    uv: channel(),
  });
  const [values, setValues] = useState([0, 0]);
  const progress = state.progress;
  const uv = progress?.uv_progress;
  useEffect(() => {
    const c = channels.current;
    if (c.task !== state.task_id) {
      c.task = state.task_id;
      c.main = channel();
      c.uv = channel();
      c.uvTask = -1;
    }
    if (uv && c.uvTask !== uv.task_id) {
      c.uv = channel();
      c.uvTask = uv.task_id;
    }
    const motion = matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;
    const draw = () => setValues([c.main.value, c.uv.value]);
    const settle = () => {
      cancelAnimationFrame(frame);
      c.main.value = c.main.target;
      c.uv.value = c.uv.target;
      draw();
    };
    const immediate =
      motion.matches ||
      document.hidden ||
      state.phase === "error" ||
      progress?.terminal === true;
    target(c.main, progress?.progress ?? 0, performance.now(), immediate);
    if (uv) target(c.uv, uv.progress, performance.now(), immediate);
    const animate = (now: number) => {
      advance(c.main, now);
      if (uv) advance(c.uv, now);
      draw();
      if (
        !document.hidden &&
        (c.main.value < c.main.target || (uv && c.uv.value < c.uv.target))
      )
        frame = requestAnimationFrame(animate);
    };
    draw();
    if (!immediate) frame = requestAnimationFrame(animate);
    motion.addEventListener("change", settle);
    document.addEventListener("visibilitychange", settle);
    return () => {
      cancelAnimationFrame(frame);
      motion.removeEventListener("change", settle);
      document.removeEventListener("visibilitychange", settle);
    };
  }, [state.task_id, state.phase, progress, uv]);
  // aria-valuenow 显式取整：Fluent 会透传原始值，测试与读屏期望整数百分比。
  const bar = (value: number, label: string, small = false) => (
    <div className={small ? styles.subProgress : styles.progress}>
      <div className={styles.progressLabel}>
        <span>{label}</span>
        <span>{Math.floor(value)}%</span>
      </div>
      <ProgressBar
        max={100}
        value={value}
        aria-label={label}
        aria-valuenow={Math.floor(value)}
      />
    </div>
  );
  return (
    <div className={styles.progressGroup}>
      {bar(values[0], progress?.title || t("progress"))}
      {uv &&
        state.phase !== "error" &&
        bar(values[1], uv.detail || t("dependencies"), true)}
    </div>
  );
}
