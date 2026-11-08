// 启动器页面内右键菜单：拦截 WebView2/WKWebView 的系统默认菜单
// （返回/另存为/打印/更多工具等浏览器动作对启动器无意义），改为跟随
// 应用深浅色主题的 Fluent 风格菜单，仅保留刷新与剪贴板相关动作。
import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { translator, type Key } from "./i18n";
import styles from "./contextMenu.module.css";

interface MenuState {
  x: number;
  y: number;
  copyable: boolean;
  editable: boolean;
}

// 当前聚焦的输入框/文本域，剪贴板动作的落点。
function editableTarget(): HTMLInputElement | HTMLTextAreaElement | null {
  const el = document.activeElement;
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    return el;
  }
  return null;
}

function selectionText(): string {
  const el = editableTarget();
  if (el) return el.value.slice(el.selectionStart ?? 0, el.selectionEnd ?? 0);
  return window.getSelection()?.toString() ?? "";
}

export function ContextMenu({
  locale,
  motion,
}: {
  locale: string;
  motion: boolean;
}) {
  const t = translator(locale);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  // 弹出后测量实际尺寸，避免贴近右/下边缘时溢出视口。
  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const rect = ref.current.getBoundingClientRect();
    const x = Math.max(8, Math.min(menu.x, window.innerWidth - rect.width - 8));
    const y = Math.max(8, Math.min(menu.y, window.innerHeight - rect.height - 8));
    if (x !== menu.x || y !== menu.y) setMenu({ ...menu, x, y });
  }, [menu]);

  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      event.preventDefault();
      setMenu({
        x: event.clientX,
        y: event.clientY,
        copyable: selectionText().length > 0,
        editable: !!editableTarget(),
      });
    };
    // 捕获阶段兜住菜单外的按下动作（对齐系统菜单“点外面就关”）；
    // 菜单内按下则阻止默认，防止 mousedown 清空待复制的文本选区。
    const onMouseDown = (event: MouseEvent) => {
      if (ref.current && event.target instanceof Node && ref.current.contains(event.target)) {
        event.preventDefault();
        return;
      }
      setMenu(null);
    };
    const onBlur = () => setMenu(null);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenu(null);
    };
    document.addEventListener("contextmenu", onContextMenu);
    document.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("contextmenu", onContextMenu);
      document.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  if (!menu) return null;

  const pasteInto = (el: HTMLInputElement | HTMLTextAreaElement, text: string) => {
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? el.value.length;
    el.setRangeText(text, start, end, "end");
    // React 受控组件依赖 input 事件感知外部修改。
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };

  const runCopy = async () => {
    const text = selectionText();
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      document.execCommand("copy");
    }
  };

  const runPaste = async () => {
    const el = editableTarget();
    if (!el) return;
    // WebView2 可能就剪贴板读取弹权限询问；拒绝或失败时静默放弃。
    try {
      const text = await navigator.clipboard.readText();
      if (text) pasteInto(el, text);
    } catch {
      /* 剪贴板权限被拒或页面未聚焦：忽略 */
    }
  };

  const runSelectAll = () => {
    const el = editableTarget();
    if (el) {
      el.select();
    } else {
      document.execCommand("selectAll");
    }
  };

  const items: { key: Key; run: () => void; enabled: boolean }[] = [
    { key: "ctx_refresh", run: () => location.reload(), enabled: true },
    { key: "ctx_copy", run: () => void runCopy(), enabled: menu.copyable },
    { key: "ctx_paste", run: () => void runPaste(), enabled: menu.editable },
    { key: "ctx_select_all", run: runSelectAll, enabled: menu.editable },
  ];

  return (
    <div
      ref={ref}
      className={styles.menu}
      role="menu"
      data-motion={motion ? "on" : "off"}
      style={{ left: menu.x, top: menu.y, animation: motion ? undefined : "none" }}
    >
      {items.map((item, index) => (
        <Fragment key={item.key}>
          {index === 1 && <div className={styles.separator} role="separator" />}
          <button
            type="button"
            role="menuitem"
            className={styles.item}
            disabled={!item.enabled}
            onClick={() => {
              setMenu(null);
              item.run();
            }}
          >
            {t(item.key)}
          </button>
        </Fragment>
      ))}
    </div>
  );
}
