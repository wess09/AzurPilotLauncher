import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
} from "@fluentui/react-components";
import { Copy16Regular } from "@fluentui/react-icons";
import styles from "./launcher.module.css";

// PCL 风格错误报告弹窗：标题 + 可滚动的原始错误日志 + 复制/关闭。
// 父组件按状态条件挂载，open 恒为 true；Escape 与 backdrop 均回调 onClose。
export function ErrorDialog({
  title,
  log,
  copyLabel,
  copiedLabel,
  closeLabel,
  onClose,
}: {
  title: string;
  log: string;
  copyLabel: string;
  copiedLabel: string;
  closeLabel: string;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(log);
      setCopied(true);
    } catch {
      /* 剪贴板不可用时静默失败 */
    }
  };
  return (
    <Dialog
      open
      modalType="alert"
      aria-labelledby="error-dialog-title"
      onOpenChange={(_event, data) => {
        if (!data.open) onClose();
      }}
    >
      <DialogSurface
        className={[styles.dialog, styles.errorDialog].filter(Boolean).join(" ")}
        aria-labelledby="error-dialog-title"
      >
        <DialogBody>
          <DialogTitle id="error-dialog-title">{title}</DialogTitle>
          <DialogContent>
            <pre className={styles.errorLog}>{log}</pre>
          </DialogContent>
          <DialogActions>
            <Button
              icon={<Copy16Regular />}
              onClick={() => void copy()}
              autoFocus
            >
              {copied ? copiedLabel : copyLabel}
            </Button>
            <Button appearance="primary" onClick={onClose}>
              {closeLabel}
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
