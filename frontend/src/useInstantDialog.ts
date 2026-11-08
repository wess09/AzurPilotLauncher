import { useEffect } from "react";

// Fluent v9 用 WAAPI（element.animate）驱动弹窗入场淡入；在休眠标签页或被
// 系统节流的渲染环境里动画时钟可能冻结在起点，弹窗与遮罩停在 opacity 0
// 造成"隐形"。WinUI 3 原生 MessageBox 本就是瞬时弹出：挂载后立即完成入场
// 动画，保证任何环境下弹窗立即可见，也更贴近系统对话框行为。
export function useInstantDialog() {
  useEffect(() => {
    const finish = () => {
      document
        .querySelectorAll<HTMLElement>(
          '[role="dialog"], [role="alertdialog"], .fui-DialogSurface__backdrop',
        )
        .forEach((el) => {
          // jsdom 等测试环境不实现 Web Animations API，缺方法时直接跳过。
          if (typeof el.getAnimations !== "function") return;
          el.getAnimations().forEach((a) => a.finish());
        });
    };
    finish();
    const raf = requestAnimationFrame(finish);
    return () => cancelAnimationFrame(raf);
  }, []);
}
