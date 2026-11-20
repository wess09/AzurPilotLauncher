import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { parse } from "yaml";

export default defineConfig({
  plugins: [
    react(),
    {
      name: "launcher-locales",
      resolveId(id) {
        if (id === "virtual:launcher-locales") return "\0launcher-locales";
      },
      load(id) {
        if (id !== "\0launcher-locales") return;
        const dictionaries = Object.fromEntries(
          ["zh-CN", "zh-TW", "ja", "en", "zh-neko"].map((locale) => {
            const path = resolve(
              import.meta.dirname,
              "../locales",
              `${locale}.yml`,
            );
            this.addWatchFile(path);
            const parsed = parse(readFileSync(path, "utf8"));
            // Tips 由主页取消按钮上方轮播展示，放进前端字典供 typed key 读取。
            const tips = Object.fromEntries(
              Object.entries(parsed.tips ?? {}).map(([index, text]) => [
                `tip_${index}`,
                text,
              ]),
            );
            return [locale, { ...parsed.launcher, ...tips }];
          }),
        );
        const keys = Object.keys(dictionaries["zh-CN"]).sort().join(",");
        for (const [locale, dictionary] of Object.entries(dictionaries)) {
          if (Object.keys(dictionary).sort().join(",") !== keys)
            throw new Error(`Launcher translation keys differ: ${locale}`);
        }
        return `export default ${JSON.stringify(dictionaries)}`;
      },
    },
  ],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    fs: { allow: [".."] },
    // Windows 下编辑器原子保存会让 chokidar 丢事件，轮询保证模块图失效可靠。
    watch: { usePolling: true, interval: 300 },
  },
  build: { target: ["chrome105", "safari15"], assetsInlineLimit: 0 },
  test: { environment: "jsdom", restoreMocks: true },
});
