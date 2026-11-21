"""从仓库内的完整字体重新生成启动器内嵌子集。

运行方式（需要 fonttools）：
    uv run --with fonttools python tools/build_font_subsets.py

MiSans 子集覆盖全部 locales 文案；GoogleSans 子集只保留拉丁字符，
由 font-family 顺序实现「英文用 GoogleSans、中文用 MiSans」的逐字回退。
泰文 / 阿拉伯文字符来自 MiSans Global 的对应语言 VF，单独成子集，
由 CSS 的 unicode-range 按需加载。
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FONTS = ROOT / "fonts"
LOCALES = ROOT / "locales"

MISANS_SRC = FONTS / "MiSansVF.ttf"
MISANS_DST = FONTS / "MiSansLauncher.ttf"
GOOGLE_SRC = FONTS / "GoogleSans-VariableFont_GRAD,opsz,wght.ttf"
GOOGLE_DST = FONTS / "GoogleSansLauncher.ttf"

# 猫娘语文案里的颜文字用到泰文与阿拉伯文字符，主 MiSans VF 不含这些字形。
THAI_SRC = FONTS / "MiSansThaiVF.ttf"
THAI_DST = FONTS / "MiSansThaiLauncher.ttf"
ARABIC_SRC = FONTS / "MiSansArabicVF.ttf"
ARABIC_DST = FONTS / "MiSansArabicLauncher.ttf"

# ASCII 必须保留：启动器里版本号、路径、日志、快捷键全是 ASCII。
ASCII = frozenset(chr(code) for code in range(0x20, 0x7F))
# 界面里常见但不在 ASCII 范围内的标点（中英混排、状态提示）。
PUNCTUATION = frozenset("…—·×÷°±≠≤≥∞•®©™¥€£←→↑↓‘’“”「」『』、。，；：？！（）【】")


def locale_chars() -> set[str]:
    """收集全部语言文件的字符，避免新增文案后再次出现缺字回退。"""
    chars = set(ASCII) | set(PUNCTUATION)
    for path in sorted(LOCALES.glob("*.yml")):
        chars.update(path.read_text(encoding="utf-8"))
    return chars


def subset(src: Path, dst: Path, chars: set[str]) -> None:
    """从 src 抽出 chars 对应的字形，保留可变字轴。"""
    if not src.is_file():
        raise SystemExit(f"缺少源字体: {src}")
    if not chars:
        return
    # --text-file 避免字符集过大时触发 Windows 命令行长度限制。
    with tempfile.NamedTemporaryFile(
        "w", encoding="utf-8", suffix=".txt", delete=False
    ) as handle:
        handle.write("".join(sorted(chars)))
        text_file = Path(handle.name)
    try:
        subprocess.run(
            [
                sys.executable,
                "-m",
                "fontTools.subset",
                str(src),
                f"--text-file={text_file}",
                f"--output-file={dst}",
                "--layout-features=*",
            ],
            check=True,
        )
    finally:
        text_file.unlink(missing_ok=True)
    print(f"{dst.name}: {len(chars)} chars, {dst.stat().st_size / 1024:.0f} KiB")


def main() -> None:
    all_chars = locale_chars()
    subset(MISANS_SRC, MISANS_DST, all_chars)

    latin = set(ASCII) | set("…—·×÷°±®©™‘’“”•")
    subset(GOOGLE_SRC, GOOGLE_DST, latin)

    # 补主 VF 不含的泰文 / 阿拉伯文字符（仅语言文案实际用到的部分）。
    thai = {ch for ch in all_chars if 0x0E00 <= ord(ch) <= 0x0E7F}
    arabic = {ch for ch in all_chars if 0x0600 <= ord(ch) <= 0x06FF}
    subset(THAI_SRC, THAI_DST, thai)
    subset(ARABIC_SRC, ARABIC_DST, arabic)


if __name__ == "__main__":
    main()
