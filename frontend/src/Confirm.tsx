import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
} from "@fluentui/react-components";
import styles from "./launcher.module.css";
import { useInstantDialog } from "./useInstantDialog";

// WinUI 3 ContentDialog 风格确认弹窗（对齐 Windows 11 标准规范）：
// 顶部内容区（标题 + 正文）+ 底部灰色分层操作栏（等宽按钮平分宽度）。
// 父组件按状态条件挂载，open 恒为 true；Escape 与点击遮罩在非 pending 时触发取消。
export function Confirm({
  title,
  body,
  confirm,
  cancel,
  pending,
  onConfirm,
  onCancel,
}: {
  title: string;
  body: string;
  confirm: string;
  cancel: string;
  pending: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useInstantDialog();
  return (
    <Dialog
      open
      modalType="alert"
      aria-labelledby="confirm-title"
      onOpenChange={(_event, data) => {
        if (!data.open && !pending) onCancel();
      }}
    >
      <DialogSurface
        className={[styles.messageBoxSurface, styles.confirmMessageBoxSurface].filter(Boolean).join(" ")}
        aria-labelledby="confirm-title"
      >
        <DialogBody className={styles.messageBoxBody}>
          <div className={styles.messageBoxContent}>
            <DialogTitle id="confirm-title" className={styles.messageBoxTitle} as="h2">
              {title}
            </DialogTitle>
            <DialogContent className={styles.messageBoxBodyText}>{body}</DialogContent>
          </div>
          <DialogActions className={`${styles.messageBoxFooter} ${styles.messageBoxFooterEqual}`}>
            <Button
              className={styles.messageBoxBtn}
              onClick={onCancel}
              disabled={pending}
              autoFocus
            >
              {cancel}
            </Button>
            <Button
              appearance="primary"
              className={styles.messageBoxPrimaryBtn}
              onClick={onConfirm}
              disabled={pending}
            >
              {confirm}
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
