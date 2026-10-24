import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Dropdown,
  Field,
  Option,
  Switch,
} from "@fluentui/react-components";
import { command } from "./bridge";
import type { LauncherSettings } from "./types";
import styles from "./launcher.module.css";

// UI 语言选项；label 用各语言母语名（惯例不翻译），value 为 locale 码。
export const languageOptions = [
  { value: "auto", label: "" },
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "ja", label: "日本語" },
  { value: "en", label: "English" },
] as const;

// 设置弹窗：开关即时生效（settings_set 保存并应用副作用），失败回滚。
export function SettingsDialog({
  title,
  platform,
  languageLabel,
  autoLanguageLabel,
  launchOnStartupLabel,
  autoStartServiceLabel,
  keepWebviewOnTrayLabel,
  closeLabel,
  unsupportedLabel,
  onLanguageChange,
  onClose,
}: {
  title: string;
  platform: string;
  languageLabel: string;
  autoLanguageLabel: string;
  launchOnStartupLabel: string;
  autoStartServiceLabel: string;
  keepWebviewOnTrayLabel: string;
  closeLabel: string;
  unsupportedLabel: string;
  onLanguageChange?: (locale: string | null) => void;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<LauncherSettings | null>(null);
  useEffect(() => {
    let disposed = false;
    const fallback: LauncherSettings = {
      launchOnStartup: false,
      autoStartService: false,
      keepWebviewOnTray: false,
      language: null,
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
      if (patch.language !== undefined)
        onLanguageChange?.(saved?.language ?? patch.language ?? null);
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
                <Field label={languageLabel}>
                  <Dropdown
                    value={
                      (settings.language ?? "auto") === "auto"
                        ? autoLanguageLabel
                        : (languageOptions.find(
                            (o) => o.value === settings.language,
                          )?.label ?? autoLanguageLabel)
                    }
                    selectedOptions={[settings.language ?? "auto"]}
                    onOptionSelect={(_e, data) => {
                      const value = String(
                        data.optionValue ?? "auto",
                      );
                      void update({
                        language: value === "auto" ? null : value,
                      });
                    }}
                  >
                    {languageOptions.map((option) => (
                      <Option key={option.value} value={option.value}>
                        {option.value === "auto"
                          ? autoLanguageLabel
                          : option.label}
                      </Option>
                    ))}
                  </Dropdown>
                </Field>
                <Switch
                  checked={settings.launchOnStartup}
                  label={
                    platform === "windows"
                      ? launchOnStartupLabel
                      : `${launchOnStartupLabel}（${unsupportedLabel}）`
                  }
                  disabled={platform !== "windows"}
                  onChange={(_e, data) => void update({ launchOnStartup: data.checked })}
                />
                <Switch
                  checked={settings.autoStartService}
                  label={autoStartServiceLabel}
                  onChange={(_e, data) => void update({ autoStartService: data.checked })}
                />
                <Switch
                  checked={settings.keepWebviewOnTray}
                  label={
                    platform === "windows"
                      ? keepWebviewOnTrayLabel
                      : `${keepWebviewOnTrayLabel}（${unsupportedLabel}）`
                  }
                  disabled={platform !== "windows"}
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
