import { readability, TinyColor } from "@ctrl/tinycolor";
import {
  createDarkTheme,
  createLightTheme,
  webDarkTheme,
  webLightTheme,
  type BrandVariants,
  type Theme,
} from "@fluentui/react-components";

// WinUI 排版：GoogleSans 负责英文与数字，MiSans 覆盖中文，Segoe UI Variable 随系统兜底。
const fontFamilyBase =
  'GoogleSans,MiSans,"Segoe UI Variable Text","Segoe UI Variable","Segoe UI","Microsoft YaHei UI","Microsoft YaHei","PingFang SC",sans-serif';
const fontFamilyMonospace =
  '"Cascadia Code",Consolas,"Courier New",monospace';

// Windows 11 强调色：暗色主题用亮蓝填充配深色文字，亮色主题用深蓝配白字。
export const launcherDarkTheme: Theme = {
  ...webDarkTheme,
  fontFamilyBase,
  fontFamilyMonospace,
  colorBrandBackground: "#4cc2ff",
  colorBrandBackgroundHover: "#62cbff",
  colorBrandBackgroundPressed: "#29a8e0",
  colorBrandBackground2: "#4cc2ff",
  colorBrandBackground2Hover: "#62cbff",
  colorBrandBackground2Pressed: "#29a8e0",
  colorBrandForeground1: "#4cc2ff",
  colorBrandForeground2: "#4cc2ff",
  colorNeutralForegroundOnBrand: "#052e45",
};

export const launcherLightTheme: Theme = {
  ...webLightTheme,
  fontFamilyBase,
  fontFamilyMonospace,
  colorBrandBackground: "#005fb8",
  colorBrandBackgroundHover: "#0067c0",
  colorBrandBackgroundPressed: "#0052ac",
  colorBrandBackground2: "#005fb8",
  colorBrandBackground2Hover: "#0067c0",
  colorBrandBackground2Pressed: "#0052ac",
  colorBrandForeground1: "#005fb8",
  colorBrandForeground2: "#005fb8",
};

export const normalizeHex = (value: string): string | null => {
  const match = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value.trim());
  if (!match) return null;
  const digits = match[1].length === 3
    ? match[1].split("").map((char) => char + char).join("")
    : match[1];
  return `#${digits.toLowerCase()}`;
};

const BRAND_STEPS = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160] as const;

function ensureTextContrast(color: TinyColor, foreground: string, mixWith: string): TinyColor {
  for (let amount = 0; amount < 100; amount++) {
    const candidate = color.mix(mixWith, amount);
    if (readability(candidate.toHexString(), foreground) >= 4.5) return candidate;
  }
  return new TinyColor(mixWith);
}

/** Fluent 的 10 最深、160 最浅。80 和 90 分别满足浅色和深色控件的文字对比度。 */
function makeBrandRamp(hex: string): BrandVariants {
  const base = new TinyColor(hex);
  const lightFill = ensureTextContrast(base, "#ffffff", "#000000");
  const darkFill = ensureTextContrast(base, webDarkTheme.colorNeutralForegroundInverted, "#ffffff");
  const ramp = {} as BrandVariants;
  for (const step of BRAND_STEPS) {
    ramp[step] = step <= 80
      ? lightFill.shade((80 - step) * 1.25).toHexString()
      : darkFill.tint((step - 90) * 1.25).toHexString();
  }
  return ramp;
}

export const defaultAccent = (light: boolean): string =>
  light ? "#005fb8" : "#4cc2ff";

/** 根据自定义强调色生成整套 Fluent 主题；无效色值回退到系统默认。 */
export function makeTheme(accent: string | null, light: boolean): Theme {
  const brand = accent ? normalizeHex(accent) : null;
  if (!brand) return light ? launcherLightTheme : launcherDarkTheme;
  const ramp = makeBrandRamp(brand);
  if (light) return { ...createLightTheme(ramp), fontFamilyBase, fontFamilyMonospace };

  return {
    ...createDarkTheme(ramp),
    fontFamilyBase,
    fontFamilyMonospace,
    colorBrandBackground: ramp[100],
    colorBrandBackgroundHover: ramp[110],
    colorBrandBackgroundPressed: ramp[90],
    colorBrandBackgroundSelected: ramp[90],
    colorNeutralForegroundOnBrand: webDarkTheme.colorNeutralForegroundInverted,
  };
}
