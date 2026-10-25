#!/usr/bin/env bash
# CI 日志脱敏辅助：把输入中所有「协议://」开头的 URL 整体替换为 <URL>。
#
# 用法（run 块内，需先声明 pipefail 以保留命令退出码）:
#   set -o pipefail
#   some-command 2>&1 | bash tools/ci/redact-urls.sh
#
# 与 workflow 里的 ::add-mask:: 掩码互补：掩码只覆盖清单内的固定地址，
# 本脚本兜底过滤 npm/apt/choco 等工具输出的任意第三方 URL。
exec sed -E -e 's@[a-zA-Z][a-zA-Z0-9+.-]*://[^[:space:]"<>()）」]+@<URL>@g' \
            -e 's@([a-zA-Z0-9-]+\.)?azurpilot\.com[^[:space:]"<>()）」]*@<URL>@g' \
            -e 's@([a-zA-Z0-9-]+\.)?cnb\.cool[^[:space:]"<>()）」]*@<URL>@g'
