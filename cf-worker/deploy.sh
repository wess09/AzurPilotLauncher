#!/usr/bin/env bash
# 把 stable.json 作为纯静态资源发布到 Cloudflare Worker。
#
# 用法（仓库根目录）:
#   bash cf-worker/deploy.sh [wrangler deploy 的额外参数]
#
# 只做本地校验、不上传:
#   bash cf-worker/deploy.sh --dry-run
#
# 该脚本不会生成 stable.json；CI 会先生成再复制到 public/updata/。
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

manifest="public/updata/stable.json"
wrangler_version="${WRANGLER_VERSION:-4.151.1}"

if [[ ! -s "${manifest}" ]]; then
  echo "错误: 缺少 ${manifest}。请先生成 stable.json 并复制到该路径。" >&2
  exit 1
fi

# 守住“纯静态”约束：一旦出现 main，请求就会进入 Worker 脚本并消耗配额。
if grep -Eq '"main"[[:space:]]*:' wrangler.jsonc; then
  echo "错误: wrangler.jsonc 不能包含 main 入口；stable.json 必须作为静态资源发布。" >&2
  exit 1
fi

# 同理，run_worker_first: true 会把请求交给 Worker 脚本。
if grep -Eq '"run_worker_first"[[:space:]]*:[[:space:]]*true' wrangler.jsonc; then
  echo "错误: wrangler.jsonc 不能启用 run_worker_first；stable.json 必须走静态资源层。" >&2
  exit 1
fi

node - "${manifest}" <<'JS'
const fs = require("node:fs");

const path = process.argv[2];
const manifest = JSON.parse(fs.readFileSync(path, "utf8"));
const platforms = manifest.platforms ? Object.keys(manifest.platforms) : [];
if (!manifest.version || platforms.length === 0) {
  throw new Error(`${path} 缺少 version 或 platforms，拒绝发布`);
}
if (manifest.runtime) {
  const runtime = manifest.runtime;
  const ids = ['adb', 'dependencies', 'frontend', 'python', 'resources', 'source'];
  if (runtime.schema !== 1 || !/^3\.14\.\d+$/.test(runtime.python) || !/^[a-f0-9]{40}$/.test(runtime.revision)) {
    throw new Error('runtime 清单格式不兼容');
  }
  if (!runtime.platforms || Object.keys(runtime.platforms).length === 0) throw new Error('runtime 缺少平台');
  for (const components of Object.values(runtime.platforms)) {
    if (!Array.isArray(components) || components.map(c => c.id).sort().join() !== ids.join()) {
      throw new Error('runtime 缺少独立组件或组件 ID 重复');
    }
    for (const component of components) {
      if (!/^[a-f0-9]{64}$/.test(component.sha256) || !Number.isSafeInteger(component.size) || component.size <= 0 || !component.urls?.length || component.urls.some(url => !url.startsWith('https://'))) {
        throw new Error('runtime 组件摘要、大小或下载地址无效');
      }
    }
  }
}
console.log(`准备发布 stable.json ${manifest.version}（${platforms.length} 个平台）`);
JS

# Wrangler 会打印 Route 域名与遥测地址，统一把 URL 替换为 <URL>
# （Route 行不带 https:// 前缀，需按本服务域名单独匹配）；
# 顶部的 pipefail 保证失败时仍以 Wrangler 的退出码结束。
WRANGLER_SEND_METRICS=false npx --yes "wrangler@${wrangler_version}" deploy "$@" 2>&1 |
  sed -E -e 's@[a-zA-Z][a-zA-Z0-9+.-]*://[^[:space:]"<>()）」]+@<URL>@g' \
         -e 's@([a-zA-Z0-9-]+\.)?azurpilot\.com[^[:space:]"<>()）」]*@<URL>@g' \
         -e 's@([a-zA-Z0-9-]+\.)?cnb\.cool[^[:space:]"<>()）」]*@<URL>@g'
