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
import {
  ArrowDownload16Regular,
  DismissCircle24Filled,
  DismissRegular,
} from "@fluentui/react-icons";
import { subscribeLogArchiveProgress } from "./bridge";
import styles from "./launcher.module.css";
import { useInstantDialog } from "./useInstantDialog";

// WinUI 3 风格错误弹窗（对齐 Windows 11 红色错误图标与灰底操作底栏）：
// 标题行带红色 DismissCircle 错误图标与右上角关闭键；
// 正文嵌入圆角代码/日志卡片与提示文案；
// 底部操作区为灰阶分层底栏，含“下载错误日志”与“关闭”主操作。
export function ErrorDialog({
  title,
  log,
  hint,
  downloadLabel,
  packingLabel,
  closeLabel,
  onDownload,
  onClose,
}: {
  title: string;
  log: string;
  hint: string;
  downloadLabel: string;
  packingLabel: string;
  closeLabel: string;
  onDownload: () => Promise<boolean>;
  onClose: () => void;
}) {
  useInstantDialog();
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
        className={[styles.messageBoxSurface, styles.errorMessageBoxSurface].filter(Boolean).join(" ")}
        aria-labelledby="error-dialog-title"
      >
        <DialogBody className={styles.messageBoxBody}>
          <div className={styles.messageBoxContent}>
            <div className={styles.messageBoxHeader}>
              <div className={styles.messageBoxTitleGroup}>
                <span className={styles.messageBoxErrorIcon} aria-hidden="true">
                  <DismissCircle24Filled />
                </span>
                <DialogTitle id="error-dialog-title" className={styles.messageBoxTitle} as="h2">
                  {title}
                </DialogTitle>
              </div>
              <Button
                appearance="subtle"
                aria-label="Dismiss"
                icon={<DismissRegular />}
                className={styles.messageBoxCloseBtn}
                onClick={onClose}
                disabled={saving}
              />
            </div>
            <DialogContent className={styles.messageBoxInnerContent}>
              {log && (
                <pre className={styles.messageBoxLogCard} data-selectable>{log}</pre>
              )}
              {hint && <p className={styles.messageBoxHint}>{hint}</p>}
              {packing && (
                <div className={styles.packProgress}>
                  <ProgressBar value={percent / 100} thickness="large" />
                  <span>
                    {packingLabel} {percent}%
                  </span>
                </div>
              )}
            </DialogContent>
          </div>
          <DialogActions className={`${styles.messageBoxFooter} ${styles.messageBoxFooterEqual}`}>
            <Button
              className={styles.messageBoxBtn}
              icon={<ArrowDownload16Regular />}
              onClick={() => void download()}
              disabled={saving}
            >
              {downloadLabel}
            </Button>
            <Button
              appearance="primary"
              className={styles.messageBoxPrimaryBtn}
              onClick={onClose}
              disabled={saving}
            >
              {closeLabel}
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
