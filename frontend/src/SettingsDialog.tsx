import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Switch,
} from "@fluentui/react-components";
import { command } from "./bridge";
import type { LauncherSettings } from "./types";
import styles from "./launcher.module.css";

// 设置弹窗：开关即时生效（settings_set 保存并应用副作用），失败回滚。
export function SettingsDialog({
  title,
  launchOnStartupLabel,
  autoStartServiceLabel,
  keepWebviewOnTrayLabel,
  closeLabel,
  onClose,
}: {
  title: string;
  launchOnStartupLabel: string;
  autoStartServiceLabel: string;
  keepWebviewOnTrayLabel: string;
  closeLabel: string;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<LauncherSettings | null>(null);
  useEffect(() => {
    let disposed = false;
    const fallback: LauncherSettings = {
      launchOnStartup: false,
      autoStartService: false,
      keepWebviewOnTray: false,
    };
    void command<LauncherSettings>("settings_get")
      .then((s) => {
        if (!disposed) setSettings(s ?? fallback);
      })
      .catch(() => {
        if (!disposed) setSettings(fallback);
      });
    return () => {
      disposed = true;
    };
  }, []);
  const update = async (patch: Partial<LauncherSettings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      const saved = await command<LauncherSettings>("settings_set", {
        settings: next,
      });
      setSettings(saved);
    } catch {
      setSettings(settings);
    }
  };
  return (
    <Dialog
      open
      modalType="alert"
      aria-labelledby="settings-title"
      onOpenChange={(_event, data) => {
        if (!data.open) onClose();
      }}
    >
      <DialogSurface className={styles.dialog} aria-labelledby="settings-title">
        <DialogBody>
          <DialogTitle id="settings-title">{title}</DialogTitle>
          <DialogContent>
            {settings && (
              <div className={styles.settingsRows}>
                <Switch
                  checked={settings.launchOnStartup}
                  label={launchOnStartupLabel}
                  onChange={(_e, data) => void update({ launchOnStartup: data.checked })}
                />
                <Switch
                  checked={settings.autoStartService}
                  label={autoStartServiceLabel}
                  onChange={(_e, data) => void update({ autoStartService: data.checked })}
                />
                <Switch
                  checked={settings.keepWebviewOnTray}
                  label={keepWebviewOnTrayLabel}
                  onChange={(_e, data) => void update({ keepWebviewOnTray: data.checked })}
                />
              </div>
            )}
          </DialogContent>
          <DialogActions>
            <Button appearance="primary" onClick={onClose}>
              {closeLabel}
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
