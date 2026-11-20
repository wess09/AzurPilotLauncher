import { readability, TinyColor } from "@ctrl/tinycolor";
import { describe, expect, it } from "vitest";
import { launcherDarkTheme, launcherLightTheme, makeTheme, normalizeHex } from "./theme";

describe("custom accent themes", () => {
  it("keeps the screenshot's red accent vivid in light mode", () => {
    const theme = makeTheme("#F00707", true);
    const fill = new TinyColor(theme.colorBrandBackground);
    expect(fill.r).toBeGreaterThan(220);
    expect(fill.g).toBeLessThan(20);
    expect(fill.getLuminance()).toBeGreaterThan(new TinyColor(theme.colorBrandBackgroundHover).getLuminance());
  });

  it.each(["#f00707", "#00ff00", "#ffff00", "#000000", "#ffffff"])(
    "keeps button labels and checked switches legible with %s",
    (accent) => {
      for (const light of [true, false]) {
        const theme = makeTheme(accent, light);
        for (const fill of [theme.colorBrandBackground, theme.colorBrandBackgroundHover, theme.colorBrandBackgroundPressed]) {
          expect(readability(fill, theme.colorNeutralForegroundOnBrand)).toBeGreaterThanOrEqual(4.5);
        }
        for (const fill of [theme.colorCompoundBrandBackground, theme.colorCompoundBrandBackgroundHover, theme.colorCompoundBrandBackgroundPressed]) {
          expect(readability(fill, theme.colorNeutralForegroundInverted)).toBeGreaterThanOrEqual(4.5);
        }
      }
    },
  );

  it("normalizes shorthand and retains each default theme when reset", () => {
    expect(normalizeHex("F07")).toBe("#ff0077");
    expect(normalizeHex("#f0")).toBeNull();
    expect(makeTheme(null, true)).toBe(launcherLightTheme);
    expect(makeTheme(null, false)).toBe(launcherDarkTheme);
  });
});
