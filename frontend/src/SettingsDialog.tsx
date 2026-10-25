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

type SourceLabel = "download" | "python" | "pypi" | "hint" | "auto" | "cnb" | "github" | "nju" | "ustc" | "npmmirror" | "aliyun" | "astral" | "pypi_name" | "tsinghua" | "tencent" | "huawei";
const sourceGroups = [
  { field: "downloadSource", label: "download", options: ["auto", "cnb", "github"] },
  { field: "pythonSource", label: "python", options: ["auto", "nju", "ustc", "npmmirror", "aliyun", "cnb", "astral"] },
  { field: "pypiSource", label: "pypi", options: ["auto", "pypi", "tsinghua", "ustc", "aliyun", "tencent", "huawei", "nju"] },
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
  reducedMotionLabel,
  preventSleepLabel,
  closeLabel,
  unsupportedLabel,
  sourceLabels,
  onLanguageChange,
  onReducedMotionChange,
  onClose,
}: {
  title: string;
  platform: string;
  languageLabel: string;
  autoLanguageLabel: string;
  launchOnStartupLabel: string;
  autoStartServiceLabel: string;
  keepWebviewOnTrayLabel: string;
  reducedMotionLabel: string;
  preventSleepLabel: string;
  closeLabel: string;
  unsupportedLabel: string;
  sourceLabels: Record<SourceLabel, string>;
  onLanguageChange?: (locale: string | null) => void;
  onReducedMotionChange?: (reduced: boolean) => void;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<LauncherSettings | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    let disposed = false;
    const fallback: LauncherSettings = {
      launchOnStartup: false,
      autoStartService: false,
      keepWebviewOnTray: false,
      reducedMotion: false,
      preventSleep: false,
      language: null,
      downloadSource: "auto",
      pythonSource: "auto",
      pypiSource: "auto",
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
    if (!settings || saving) return;
    setSaving(true);
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      const saved = await command<LauncherSettings>("settings_set", {
        settings: next,
      });
      setSettings(saved);
      if (patch.language !== undefined)
        onLanguageChange?.(saved?.language ?? patch.language ?? null);
      if (patch.reducedMotion !== undefined)
        onReducedMotionChange?.(
          saved?.reducedMotion ?? patch.reducedMotion ?? false,
        );
    } catch {
      setSettings(settings);
    } finally { setSaving(false); }
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
          <DialogContent className={styles.settingsContent}>
            {settings && (
              <div className={styles.settingsRows}>
                <Field label={languageLabel}>
                  <Dropdown disabled={saving}
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
                <div className={styles.sourceSettings}>
                  {sourceGroups.map((group) => {
                    const selected = settings[group.field] ?? "auto";
                    const name = (value: string) => sourceLabels[(value === "pypi" ? "pypi_name" : value) as SourceLabel];
                    return <Field key={group.field} label={sourceLabels[group.label]}>
                      <Dropdown disabled={saving} value={name(selected)} selectedOptions={[selected]}
                        onOptionSelect={(_event, data) => void update({ [group.field]: data.optionValue ?? "auto" })}>
                        {group.options.map((value) => <Option key={value} value={value}>{name(value)}</Option>)}
                      </Dropdown>
                    </Field>;
                  })}
                  <p className={styles.sourceHint}>{sourceLabels.hint}</p>
                </div>
                <Switch
                  checked={settings.launchOnStartup}
                  label={
                    platform === "windows"
                      ? launchOnStartupLabel
                      : `${launchOnStartupLabel}（${unsupportedLabel}）`
                  }
                  disabled={saving || platform !== "windows"}
                  onChange={(_e, data) => void update({ launchOnStartup: data.checked })}
                />
                <Switch
                  disabled={saving}
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
                  disabled={saving || platform !== "windows"}
                  onChange={(_e, data) => void update({ keepWebviewOnTray: data.checked })}
                />
                <Switch
                  disabled={saving}
                  checked={settings.reducedMotion}
                  label={reducedMotionLabel}
                  onChange={(_e, data) => void update({ reducedMotion: data.checked })}
                />
                <Switch
                  disabled={saving}
                  checked={settings.preventSleep}
                  label={preventSleepLabel}
                  onChange={(_e, data) => void update({ preventSleep: data.checked })}
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
