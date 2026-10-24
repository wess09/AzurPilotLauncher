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

// Fluent ContentDialog 形态：父组件按状态条件挂载，open 恒为 true；
// Escape 触发的 onOpenChange 与 backdrop 一样只在非 pending 时回调取消。
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
  return (
    <Dialog
      open
      modalType="alert"
      aria-labelledby="confirm-title"
      onOpenChange={(_event, data) => {
        if (!data.open && !pending) onCancel();
      }}
    >
      <DialogSurface className={styles.dialog} aria-labelledby="confirm-title">
        <DialogBody>
          <DialogTitle id="confirm-title">{title}</DialogTitle>
          <DialogContent>{body}</DialogContent>
          <DialogActions>
            <Button onClick={onCancel} disabled={pending} autoFocus>
              {cancel}
            </Button>
            <Button
              appearance="primary"
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
