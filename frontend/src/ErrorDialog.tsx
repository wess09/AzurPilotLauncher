import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  ProgressBar,
} from "@fluentui/react-components";
import { ArrowDownload16Regular } from "@fluentui/react-icons";
import { subscribeLogArchiveProgress } from "./bridge";
import styles from "./launcher.module.css";

// PCL 风格错误报告弹窗：标题 + 可滚动的原始错误日志 + 下载日志归档/关闭。
// 打包在后台线程执行，进度经 launcher://log-archive-progress 推送，
// 对话框内以进度条呈现；按钮文字恒定，仅保存中禁用。
export function ErrorDialog({
  title,
  log,
  downloadLabel,
  packingLabel,
  closeLabel,
  onDownload,
  onClose,
}: {
  title: string;
  log: string;
  downloadLabel: string;
  packingLabel: string;
  closeLabel: string;
  onDownload: () => Promise<boolean>;
  onClose: () => void;
}) {
  const [saving, setSaving] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  useEffect(() => {
    let disposed = false;
    void subscribeLogArchiveProgress((p) => {
      if (!disposed) setProgress(p);
    }).then((unlisten) => {
      if (disposed) unlisten();
    });
    return () => {
      disposed = true;
    };
  }, []);
  const download = async () => {
    setSaving(true);
    await onDownload();
    setSaving(false);
    setProgress(null);
  };
  const packing = saving && progress != null;
  const percent = progress && progress.total > 0
    ? Math.min(100, Math.round((progress.done / progress.total) * 100))
    : 0;
  return (
    <Dialog
      open
      modalType="alert"
      aria-labelledby="error-dialog-title"
      onOpenChange={(_event, data) => {
        if (!data.open && !saving) onClose();
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
            {packing && (
              <div className={styles.packProgress}>
                <ProgressBar value={percent / 100} thickness="large" />
                <span>
                  {packingLabel} {percent}%
                </span>
              </div>
            )}
          </DialogContent>
          <DialogActions>
            <Button
              icon={<ArrowDownload16Regular />}
              onClick={() => void download()}
              disabled={saving}
            >
              {downloadLabel}
            </Button>
            <Button appearance="primary" onClick={onClose} disabled={saving}>
              {closeLabel}
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
