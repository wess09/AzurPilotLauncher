import { useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
} from "@fluentui/react-components";
import { ArrowDownload16Regular } from "@fluentui/react-icons";
import styles from "./launcher.module.css";

// PCL 风格错误报告弹窗：标题 + 可滚动的原始错误日志 + 下载日志归档/关闭。
// 父组件按状态条件挂载，open 恒为 true；Escape 与 backdrop 均回调 onClose。
export function ErrorDialog({
  title,
  log,
  downloadLabel,
  savedLabel,
  closeLabel,
  onDownload,
  onClose,
}: {
  title: string;
  log: string;
  downloadLabel: string;
  savedLabel: string;
  closeLabel: string;
  onDownload: () => Promise<boolean>;
  onClose: () => void;
}) {
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const download = async () => {
    setSaving(true);
    const ok = await onDownload();
    setSaving(false);
    setSaved(ok);
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
              icon={<ArrowDownload16Regular />}
              onClick={() => void download()}
              disabled={saving}
            >
              {saved ? savedLabel : downloadLabel}
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
