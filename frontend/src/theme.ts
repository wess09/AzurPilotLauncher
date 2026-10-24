import {
  webDarkTheme,
  webLightTheme,
  type Theme,
} from "@fluentui/react-components";

// WinUI 排版：内嵌 MiSans 优先覆盖中西文，Segoe UI Variable 随系统兜底。
const fontFamilyBase =
  'MiSans,"Segoe UI Variable Text","Segoe UI Variable","Segoe UI","Microsoft YaHei UI","Microsoft YaHei","PingFang SC",sans-serif';
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
