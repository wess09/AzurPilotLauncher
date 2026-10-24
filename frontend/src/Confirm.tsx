import { useEffect, useRef } from "react";
import styles from "./launcher.module.css";

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
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current!;
    dialog.showModal();
    return () => dialog.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className={styles.dialog}
      aria-labelledby="confirm-title"
      onCancel={(e) => {
        e.preventDefault();
        if (!pending) onCancel();
      }}
    >
      <h2 id="confirm-title">{title}</h2>
      <p>{body}</p>
      <div className={styles.dialogActions}>
        <button autoFocus onClick={onCancel} disabled={pending}>
          {cancel}
        </button>
        <button
          className={styles.primary}
          onClick={onConfirm}
          disabled={pending}
        >
          {confirm}
        </button>
      </div>
    </dialog>
  );
}
