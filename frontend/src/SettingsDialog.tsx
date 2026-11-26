import { useEffect, useState, type ComponentType, type ReactNode } from "react";
import {
  Accessibility16Regular,
  ArrowDownload16Regular,
  ArrowSync16Regular,
  ArrowSyncOff16Regular,
  Box16Regular,
  Braces16Regular,
  Code16Regular,
  Color16Regular,
  Flash16Regular,
  Globe16Regular,
  Image16Regular,
  Open16Regular,
  OptionsSpeaker16Regular,
  Play16Regular,
  Power20Regular,
  Video16Regular,
  VideoProhibited16Regular,
  Window16Regular,
  Wrench16Regular,
} from "@fluentui/react-icons";
import {
  Button,
  ColorArea,
  ColorPicker,
  ColorSlider,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Dropdown,
  Input,
  Menu,
  MenuItem,
  MenuList,
  MenuPopover,
  MenuTrigger,
  Option,
  Popover,
  PopoverSurface,
  PopoverTrigger,
  SplitButton,
  Switch,
  type ColorPickerProps,
} from "@fluentui/react-components";
import { command, SETTINGS_CHANGED_EVENT } from "./bridge";
import { TinyColor } from "@ctrl/tinycolor";
import type { LauncherSettings } from "./types";
import { normalizeHex } from "./theme";
import styles from "./launcher.module.css";
import { useInstantDialog } from "./useInstantDialog";

// UI 语言选项；label 用各语言母语名（惯例不翻译），value 为 locale 码。
export const languageOptions = [
  { value: "auto", label: "" },
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "ja", label: "日本語" },
  { value: "en", label: "English" },
  { value: "zh-neko", label: "猫娘语 (ฅ'ω'ฅ)" },
] as const;

const updateSourceOptions = ["auto", "mirrorchyan", "cnb", "gitcode", "official", "cloudflare", "github"] as const;

type SourceLabel = "download" | "python" | "nodejs" | "pypi" | "auto" | "official" | "cnb" | "github" | "nju" | "ustc" | "npmmirror" | "aliyun" | "astral" | "pypi_name" | "tsinghua" | "tencent" | "huawei";
const sourceGroups = [
  { field: "downloadSource", label: "download", icon: Wrench16Regular, options: ["auto", "cnb", "github"] },
  { field: "pythonSource", label: "python", icon: Braces16Regular, options: ["auto", "nju", "ustc", "npmmirror", "aliyun", "cnb", "astral"] },
  { field: "nodejsSource", label: "nodejs", icon: Code16Regular, options: ["auto", "npmmirror", "nju", "official"] },
  { field: "pypiSource", label: "pypi", icon: Box16Regular, options: ["auto", "pypi", "tsinghua", "ustc", "aliyun", "tencent", "huawei", "nju"] },
] as const;

// 设置项：图标 + 标题 + 右侧控件，不附加说明文字。
// 标题同时作为控件的 aria-label，图标由 SVG 自带 aria-hidden。
function SettingCard({
  icon: Icon,
  title,
  status,
  children,
}: {
  icon: ComponentType;
  title: string;
  status?: string;
  children?: ReactNode;
}) {
  return (
    <div className={styles.settingCard}>
      <span className={styles.settingCardIcon}><Icon /></span>
      <div className={styles.settingCardTitle}>
        {title}
        {status && <div className={styles.settingCardStatus} role="status">{status}</div>}
      </div>
      {children && <div className={styles.settingCardControl}>{children}</div>}
    </div>
  );
}

// 设置弹窗：开关即时生效（settings_set 保存并应用副作用），失败回滚。
export function SettingsDialog({
  title,
  platform,
  interfaceGroupLabel,
  updateGroupLabel,
  sourcesGroupLabel,
  startupGroupLabel,
  languageLabel,
  autoLanguageLabel,
  accentColorLabel,
  defaultAccentColor,
  customBackgroundImageLabel,
  customBackgroundVideoLabel,
  backgroundVideoStatus,
  selectFileLabel,
  clearLabel,
  resetLabel,
  applyLabel,
  cancelLabel,
  updateChannelLabel,
  channelStableLabel,
  channelCanaryLabel,
  updateSourceLabel,
  updateSourceLabels,
  useMirrorChyanLabel,
  mirrorChyanCdkLabel,
  mirrorChyanCdkPlaceholder,
  mirrorChyanWebsiteLabel,
  launchOnStartupLabel,
  autoStartServiceLabel,
  keepWebviewOnTrayLabel,
  reducedMotionLabel,
  disableBackgroundVideoLabel,
  preventSleepLabel,
  audioEnabledLabel,
  skipUpdateLabel,
  closeLabel,
  unsupportedLabel,
  sourceLabels,
  onLanguageChange,
  onReducedMotionChange,
  onAudioChange,
  onDisableBackgroundVideoChange,
  onCustomBackgroundVideoChange,
  onClose,
}: {
  title: string;
  platform: string;
  interfaceGroupLabel: string;
  updateGroupLabel: string;
  sourcesGroupLabel: string;
  startupGroupLabel: string;
  languageLabel: string;
  autoLanguageLabel: string;
  accentColorLabel: string;
  defaultAccentColor: string;
  customBackgroundImageLabel: string;
  customBackgroundVideoLabel: string;
  backgroundVideoStatus?: string;
  selectFileLabel: string;
  clearLabel: string;
  resetLabel: string;
  applyLabel: string;
  cancelLabel: string;
  updateChannelLabel: string;
  channelStableLabel: string;
  channelCanaryLabel: string;
  updateSourceLabel: string;
  updateSourceLabels: Record<LauncherSettings["updateSource"], string>;
  useMirrorChyanLabel: string;
  mirrorChyanCdkLabel: string;
  mirrorChyanCdkPlaceholder: string;
  mirrorChyanWebsiteLabel: string;
  launchOnStartupLabel: string;
  autoStartServiceLabel: string;
  keepWebviewOnTrayLabel: string;
  reducedMotionLabel: string;
  disableBackgroundVideoLabel: string;
  preventSleepLabel: string;
  audioEnabledLabel: string;
  skipUpdateLabel: string;
  closeLabel: string;
  unsupportedLabel: string;
  sourceLabels: Record<SourceLabel, string>;
  onLanguageChange?: (locale: string | null) => void;
  onReducedMotionChange?: (reduced: boolean) => void;
  onAudioChange?: (enabled: boolean) => void;
  onDisableBackgroundVideoChange?: (disabled: boolean) => void;
  onCustomBackgroundVideoChange?: (path: string | null) => void;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<LauncherSettings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [colorOpen, setColorOpen] = useState(false);
  const [colorDraft, setColorDraft] = useState(() => new TinyColor(defaultAccentColor).toHsv());
  const [hexDraft, setHexDraft] = useState(defaultAccentColor);
  const [cdkDraft, setCdkDraft] = useState("");
  useInstantDialog();
  useEffect(() => {
    let disposed = false;
    const fallback: LauncherSettings = {
      launchOnStartup: false,
      autoStartService: false,
      keepWebviewOnTray: false,
      reducedMotion: false,
      disableBackgroundVideo: false,
      preventSleep: false,
      audioEnabled: false,
      skipUpdate: false,
      updateChannel: "stable",
      updateSource: "auto",
      language: null,
      accentColor: null,
      customBackgroundImage: null,
      customBackgroundVideo: null,
      downloadSource: "auto",
      pythonSource: "auto",
      nodejsSource: "auto",
      pypiSource: "auto",
      useMirrorChyan: false,
      mirrorChyanCdk: null,
    };
    void command<LauncherSettings>("settings_get")
      .then((s) => {
        if (!disposed) {
          const resolved = s ?? fallback;
          setSettings(resolved);
          setCdkDraft(resolved.mirrorChyanCdk ?? "");
        }
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
    setError("");
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
      if (patch.audioEnabled !== undefined)
        onAudioChange?.(saved?.audioEnabled ?? patch.audioEnabled ?? false);
      if (patch.disableBackgroundVideo !== undefined)
        onDisableBackgroundVideoChange?.(
          saved?.disableBackgroundVideo ?? patch.disableBackgroundVideo ?? false,
        );
      if (patch.customBackgroundVideo !== undefined)
        onCustomBackgroundVideoChange?.(saved?.customBackgroundVideo ?? patch.customBackgroundVideo);
      if (patch.accentColor !== undefined || patch.customBackgroundImage !== undefined) {
        window.dispatchEvent(new Event(SETTINGS_CHANGED_EVENT));
      }
    } catch (error) {
      setSettings(settings);
      setError(String(error));
    } finally { setSaving(false); }
  };
  const pickBackground = async (kind: "image" | "video") => {
    if (!settings || saving) return;
    setSaving(true);
    setError("");
    try {
      const path = await command<string | null>("pick_background_file", { kind });
      if (path) {
        await update(
          kind === "image"
            ? { customBackgroundImage: path }
            : { customBackgroundVideo: path, disableBackgroundVideo: false },
        );
      }
    } catch (error) {
      setError(String(error));
    } finally {
      setSaving(false);
    }
  };
  const accentValue = normalizeHex(settings?.accentColor ?? "") ?? defaultAccentColor;
  const parsedHexDraft = normalizeHex(hexDraft);
  const onColorChange: ColorPickerProps["onColorChange"] = (_event, data) => {
    const color = { ...data.color, a: data.color.a ?? 1 };
    setColorDraft(color);
    setHexDraft(new TinyColor(color).toHexString());
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
            {error && <p role="alert" data-selectable>{error}</p>}
            {settings && (
              <div className={styles.settingsRows}>
                <div className={styles.settingsGroupTitle}>{interfaceGroupLabel}</div>
                <SettingCard icon={Globe16Regular} title={languageLabel}>
                  <Dropdown aria-label={languageLabel} disabled={saving}
                    value={
                      (settings.language ?? "auto") === "auto"
                        ? autoLanguageLabel
                        : (languageOptions.find(
                            (o) => o.value === settings.language,
                          )?.label ?? autoLanguageLabel)
                    }
                    selectedOptions={[settings.language ?? "auto"]}
                    onOptionSelect={(_e, data) => {
                      const value = String(data.optionValue ?? "auto");
                      void update({ language: value === "auto" ? null : value });
                    }}
                  >
                    {languageOptions.map((option) => (
                      <Option key={option.value} value={option.value}>
                        {option.value === "auto" ? autoLanguageLabel : option.label}
                      </Option>
                    ))}
                  </Dropdown>
                </SettingCard>
                <SettingCard icon={Color16Regular} title={accentColorLabel}>
                  <div className={styles.settingActions}>
                    <Popover size="small" open={colorOpen} onOpenChange={(_event, data) => {
                      if (data.open) {
                        setColorDraft(new TinyColor(accentValue).toHsv());
                        setHexDraft(accentValue);
                      }
                      setColorOpen(data.open);
                    }}>
                      <PopoverTrigger>
                        <Button size="small" className={styles.settingActionMain}
                          disabled={saving} aria-label={accentColorLabel}>
                          <span className={styles.settingColorDot}
                            style={{ backgroundColor: accentValue }} aria-hidden="true" />
                          <span className={styles.settingHexText}>{accentValue.toUpperCase()}</span>
                        </Button>
                      </PopoverTrigger>
                      <PopoverSurface className={styles.colorPopover}>
                        <ColorPicker color={colorDraft} onColorChange={onColorChange}>
                          <ColorSlider />
                          <ColorArea />
                        </ColorPicker>
                        <Input aria-label={accentColorLabel} value={hexDraft}
                          aria-invalid={!parsedHexDraft}
                          onChange={(_event, data) => {
                            setHexDraft(data.value);
                            const hex = normalizeHex(data.value);
                            if (hex) setColorDraft(new TinyColor(hex).toHsv());
                          }} />
                        <div className={styles.colorPopoverActions}>
                          <Button size="small" disabled={saving || !settings.accentColor}
                            onClick={() => {
                              void update({ accentColor: null });
                              setColorOpen(false);
                            }}>
                            {resetLabel}
                          </Button>
                          <Button size="small" onClick={() => setColorOpen(false)}>{cancelLabel}</Button>
                          <Button appearance="primary" size="small" disabled={saving || !parsedHexDraft}
                            onClick={() => {
                              if (!parsedHexDraft) return;
                              void update({ accentColor: parsedHexDraft });
                              setColorOpen(false);
                            }}>
                            {applyLabel}
                          </Button>
                        </div>
                      </PopoverSurface>
                    </Popover>
                  </div>
                </SettingCard>
                <SettingCard icon={Image16Regular} title={customBackgroundImageLabel}>
                  <Menu>
                    <MenuTrigger disableButtonEnhancement>
                      {(menuButton) => (
                        <SplitButton size="small" className={styles.settingActionMain}
                          menuButton={menuButton}
                          primaryActionButton={{
                            "aria-label": customBackgroundImageLabel,
                            onClick: () => void pickBackground("image"),
                          }}
                          disabled={saving}>{selectFileLabel}</SplitButton>
                      )}
                    </MenuTrigger>
                    <MenuPopover>
                      <MenuList>
                        <MenuItem disabled={!settings.customBackgroundImage}
                          onClick={() => void update({ customBackgroundImage: null })}>
                          {clearLabel}
                        </MenuItem>
                      </MenuList>
                    </MenuPopover>
                  </Menu>
                </SettingCard>
                <SettingCard icon={Video16Regular} title={customBackgroundVideoLabel}
                  status={backgroundVideoStatus}>
                  <Menu>
                    <MenuTrigger disableButtonEnhancement>
                      {(menuButton) => (
                        <SplitButton size="small" className={styles.settingActionMain}
                          menuButton={menuButton}
                          primaryActionButton={{
                            "aria-label": customBackgroundVideoLabel,
                            onClick: () => void pickBackground("video"),
                          }}
                          disabled={saving}>{selectFileLabel}</SplitButton>
                      )}
                    </MenuTrigger>
                    <MenuPopover>
                      <MenuList>
                        <MenuItem disabled={!settings.customBackgroundVideo}
                          onClick={() => void update({ customBackgroundVideo: null })}>
                          {clearLabel}
                        </MenuItem>
                      </MenuList>
                    </MenuPopover>
                  </Menu>
                </SettingCard>
                <SettingCard icon={Accessibility16Regular} title={reducedMotionLabel}>
                  <Switch aria-label={reducedMotionLabel} disabled={saving}
                    checked={settings.reducedMotion}
                    onChange={(_e, data) => void update({ reducedMotion: data.checked })} />
                </SettingCard>
                <SettingCard icon={OptionsSpeaker16Regular} title={audioEnabledLabel}>
                  <Switch aria-label={audioEnabledLabel} disabled={saving}
                    checked={settings.audioEnabled}
                    onChange={(_e, data) => void update({ audioEnabled: data.checked })} />
                </SettingCard>
                <SettingCard icon={VideoProhibited16Regular} title={disableBackgroundVideoLabel}>
                  <Switch aria-label={disableBackgroundVideoLabel} disabled={saving}
                    checked={settings.disableBackgroundVideo}
                    onChange={(_e, data) => void update({ disableBackgroundVideo: data.checked })} />
                </SettingCard>
                <div className={styles.settingsGroupTitle}>{updateGroupLabel}</div>
                <SettingCard icon={ArrowSync16Regular} title={updateChannelLabel}>
                  <Dropdown aria-label={updateChannelLabel} disabled={saving}
                    value={settings.updateChannel === "canary" ? channelCanaryLabel : channelStableLabel}
                    selectedOptions={[settings.updateChannel ?? "stable"]}
                    onOptionSelect={(_e, data) =>
                      void update({
                        updateChannel: data.optionValue === "canary" ? "canary" : "stable",
                      })
                    }
                  >
                    <Option value="stable">{channelStableLabel}</Option>
                    <Option value="canary">{channelCanaryLabel}</Option>
                  </Dropdown>
                </SettingCard>
                <SettingCard icon={ArrowDownload16Regular} title={updateSourceLabel}>
                  <Dropdown aria-label={updateSourceLabel} disabled={saving}
                    value={updateSourceLabels[settings.updateSource ?? "auto"]}
                    selectedOptions={[settings.updateSource ?? "auto"]}
                    onOptionSelect={(_e, data) =>
                      void update({
                        updateSource: updateSourceOptions.find((source) => source === data.optionValue) ?? "auto",
                      })
                    }
                  >
                    {updateSourceOptions.map((value) => (
                      <Option key={value} value={value}>{updateSourceLabels[value]}</Option>
                    ))}
                  </Dropdown>
                </SettingCard>
                <SettingCard icon={ArrowSyncOff16Regular} title={skipUpdateLabel}>
                  <Switch aria-label={skipUpdateLabel} disabled={saving}
                    checked={settings.skipUpdate}
                    onChange={(_e, data) => void update({ skipUpdate: data.checked })} />
                </SettingCard>
                <SettingCard icon={ArrowDownload16Regular} title={useMirrorChyanLabel}>
                  <Switch aria-label={useMirrorChyanLabel} disabled={saving}
                    checked={settings.useMirrorChyan}
                    onChange={(_e, data) => void update({ useMirrorChyan: data.checked })} />
                </SettingCard>
                <SettingCard icon={Wrench16Regular} title={mirrorChyanCdkLabel}>
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <Input
                      size="small"
                      type="password"
                      aria-label={mirrorChyanCdkLabel}
                      placeholder={mirrorChyanCdkPlaceholder}
                      value={cdkDraft}
                      disabled={saving}
                      onChange={(_e, data) => setCdkDraft(data.value)}
                    />
                    <Button
                      size="small"
                      aria-label={`${applyLabel} ${mirrorChyanCdkLabel}`}
                      disabled={saving}
                      onClick={() =>
                        void update({
                          mirrorChyanCdk: cdkDraft.trim() ? cdkDraft.trim() : null,
                        })
                      }
                    >
                      {applyLabel}
                    </Button>
                    <Button
                      size="small"
                      appearance="subtle"
                      icon={<Open16Regular />}
                      aria-label={mirrorChyanWebsiteLabel}
                      title={mirrorChyanWebsiteLabel}
                      onClick={() => void command("settings_open_mirror_chyan").catch((error) => setError(String(error)))}
                    />
                  </div>
                </SettingCard>
                <div className={styles.settingsGroupTitle}>{sourcesGroupLabel}</div>
                {sourceGroups.map((group) => {
                  const selected = settings[group.field] ?? "auto";
                  const name = (value: string) => sourceLabels[(value === "pypi" ? "pypi_name" : value) as SourceLabel];
                  const label = sourceLabels[group.label];
                  return (
                    <SettingCard key={group.field} icon={group.icon} title={label}>
                      <Dropdown aria-label={label} disabled={saving} value={name(selected)} selectedOptions={[selected]}
                        onOptionSelect={(_event, data) => void update({ [group.field]: data.optionValue ?? "auto" })}>
                        {group.options.map((value) => <Option key={value} value={value}>{name(value)}</Option>)}
                      </Dropdown>
                    </SettingCard>
                  );
                })}
                <div className={styles.settingsGroupTitle}>{startupGroupLabel}</div>
                <SettingCard icon={Flash16Regular}
                  title={platform === "windows" ? launchOnStartupLabel : `${launchOnStartupLabel}（${unsupportedLabel}）`}>
                  <Switch aria-label={launchOnStartupLabel}
                    checked={settings.launchOnStartup}
                    disabled={saving || platform !== "windows"}
                    onChange={(_e, data) => void update({ launchOnStartup: data.checked })} />
                </SettingCard>
                <SettingCard icon={Play16Regular} title={autoStartServiceLabel}>
                  <Switch aria-label={autoStartServiceLabel} disabled={saving}
                    checked={settings.autoStartService}
                    onChange={(_e, data) => void update({ autoStartService: data.checked })} />
                </SettingCard>
                <SettingCard icon={Window16Regular}
                  title={platform === "windows" ? keepWebviewOnTrayLabel : `${keepWebviewOnTrayLabel}（${unsupportedLabel}）`}>
                  <Switch aria-label={keepWebviewOnTrayLabel}
                    checked={settings.keepWebviewOnTray}
                    disabled={saving || platform !== "windows"}
                    onChange={(_e, data) => void update({ keepWebviewOnTray: data.checked })} />
                </SettingCard>
                <SettingCard icon={Power20Regular} title={preventSleepLabel}>
                  <Switch aria-label={preventSleepLabel} disabled={saving}
                    checked={settings.preventSleep}
                    onChange={(_e, data) => void update({ preventSleep: data.checked })} />
                </SettingCard>
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
