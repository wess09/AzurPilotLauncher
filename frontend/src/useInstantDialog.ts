import { useEffect } from "react";

// Fluent v9 用 WAAPI（element.animate）驱动弹窗入场淡入与缩放。
// 正常前台渲染保留这段动画；仅在用户开启「减弱动态效果」或页面在后台时
// 立即完成动画——休眠标签页的动画时钟可能冻结在起点，导致弹窗与遮罩停在
// opacity 0 变成"隐形"，这两种情况下瞬时显示更符合预期。
export function useInstantDialog() {
  useEffect(() => {
    const skipMotion = () =>
      document.hidden ||
      document.documentElement.classList.contains("reduced-motion");
    if (!skipMotion()) return;
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
