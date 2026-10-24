# Cloudflare Worker：启动器更新清单（纯静态）

`cf-worker/` 只做一件事：把 `stable.json` 作为**静态文件**提供给启动器。

- `wrangler.jsonc` **没有 `main` 入口**，项目里没有任何 `fetch` 处理器（Wrangler 会为 assets-only Worker 生成一个内部 no-op 脚本，但静态资源请求不会进入它）。
- 命中静态资源的请求由 Cloudflare assets 层直接返回，不执行 Worker 脚本，**不消耗 Workers 请求配额**（静态资源请求免费且不计入调用次数）。
- 未命中的路径由 `public/404.html` 静态返回（`not_found_handling: "404-page"`），同样不会落到 Worker 脚本。
- **不要**添加 `main`、`run_worker_first: true`、中间件或任何 `fetch` 处理器，否则请求会开始消耗配额。

## 目录结构

```
cf-worker/
├── wrangler.jsonc        # Worker 配置（无 main）
├── deploy.sh             # 发布脚本：校验 stable.json 后 wrangler deploy
└── public/
    ├── 404.html          # 静态 404（避免未命中请求进入 Worker）
    ├── _headers          # stable.json 的 Cache-Control
    └── updata/
        └── stable.json   # 由 CI 生成，已 gitignore
```

## 路由

新地址：

```
https://launcher-api.azurpilot.com/updata/stable.json
```

旧地址 `https://ap-launcher-update.nanoda.work/updata/stable.json` 由旧域名侧配置 **302** 跳转到新地址；Worker 只绑定新域名，只接管 `/updata/*`，同一域名下的其他路径仍然回源。配置位于 `wrangler.jsonc`：

```jsonc
"routes": [
  { "pattern": "launcher-api.azurpilot.com/updata/*", "zone_name": "azurpilot.com" }
]
```

前提：

- `azurpilot.com` 与 Worker 在同一个 Cloudflare 账号中；
- `launcher-api.azurpilot.com` 已代理（橙色云）到 Cloudflare，Worker Route 只对代理流量生效；
- 旧地址的 302 在旧域名侧完成，不需要 Worker 参与；
- 清单按公开数据设计（启动器拉清单时不会携带 mTLS 客户端证书），无需为 Worker 配置客户端证书；如果该主机名额外启用了 mTLS / Access，Worker Route 会沿用边缘策略。`workers_dev: false` 可避免从 `*.workers.dev` 绕过这些策略；
- 如果账号里已经存在同名 Worker（`launcher-api-static`），请先修改 `wrangler.jsonc` 的 `name`，避免覆盖现有服务。

## CI 发布

`package.yml` 的 `deploy-launcher-update` 任务在 tag 构建成功后：

1. 生成 `launcher-updater-r2/stable.json`，载荷 URL 为 `https://launcher-download.azurpilot.com/<版本化文件名>`；
2. 上传版本化载荷到 R2（保留作备份）；
3. 上传同一批载荷到 CNB Release `1.0.0`，同名附件先删除再上传，实现固定覆盖；
4. 把清单复制到 `cf-worker/public/updata/stable.json`；
5. 执行 `bash cf-worker/deploy.sh`，以新版本静态资源整体替换 Worker；
6. 轮询 `NEW_UPDATE_URL`，确认 Worker 返回的 `version` 与新 tag 一致。

需要配置三个 GitHub Secrets：

| Secret | 用途 |
|---|---|
| `CLOUDFLARE_API_TOKEN` | Cloudflare API Token |
| `CLOUDFLARE_ACCOUNT_ID` | Cloudflare 账号 ID |
| `CNB_TOKEN` | CNB 访问令牌，用于上传 Release 附件 |

`CNB_TOKEN` 在 <https://cnb.cool/profile/token> 创建。`git-cnb` 官方建议的权限包括 `repo-notes:rw`、`repo-code:rw`、`repo-pr:rw`、`repo-basic-info:r`、`account-profile:r`、`group-manage:rw`；仅上传 Release 附件时至少需要 Release 与仓库信息的读写权限。

CNB Release tag 固定为 `1.0.0`，不要随应用版本变化。清单里的载荷 URL 会经 `launcher-download.azurpilot.com` 302 到：

```
https://cnb.cool/AzurPilot/AzurPilotLauncher/-/releases/download/1.0.0/<文件名>
```

例如：

```
https://launcher-download.azurpilot.com/azurpilot-windows-aarch64-2.2.5.exe
```

Token 建议使用 **Edit Cloudflare Workers** 模板，并限制到目标账号与 `azurpilot.com` 区域。至少需要：

- Account → Workers Scripts → Edit
- Zone → Workers Routes → Edit
- Zone → Zone → Read（Wrangler 按 `zone_name` 查找 zone）

如果不想让 CI 管理路由，可以删除 `wrangler.jsonc` 的 `routes`，改在 Cloudflare 控制台手动绑定 Worker；此时 token 只需 Workers Scripts: Edit。

旧版 SSH 清单任务（`deploy-launcher-update-legacy`）已删除；`NEW_MANIFEST_DEPLOY_ROOT` 和 `DEPLOY_*` 系列 secret 不再被任何 workflow 使用，可以从仓库 Secrets 中删除。

## 手动发布

```bash
# 1. 准备清单（CI 中由 Python 生成）
mkdir -p cf-worker/public/updata
cp /path/to/stable.json cf-worker/public/updata/stable.json

# 2. 校验并部署
bash cf-worker/deploy.sh

# 只做本地校验，不上传
bash cf-worker/deploy.sh --dry-run
```

`deploy.sh` 会拒绝缺失或无效的 `stable.json`，并在配置里发现 `main` 或 `run_worker_first: true` 时直接报错。

本地预览：

```bash
cd cf-worker
npx --yes wrangler@4.151.1 dev
```

## 缓存

`public/_headers` 给 `/updata/stable.json` 设置 `Cache-Control: no-cache`，客户端每次使用前会带 ETag 重新校验；新版本部署后立即可见。

## 切换域名 / 回滚

- 整个域名交给 Worker：把 route 改成 `launcher-api.azurpilot.com/*`，或使用 `custom_domain: true`（Wrangler 会管理 DNS，需要先移除冲突记录）。
- 使用 `*.workers.dev`：把 `workers_dev` 改为 `true`，并同步修改 `package.yml` 的 `NEW_UPDATE_URL`、构建步骤的 `LAUNCHER_UPDATE_URL` 以及 `build.rs` 的默认地址；需要时同步 `src/main.rs` 的回退地址。
- 旧地址 `ap-launcher-update.nanoda.work` 的 302 在旧域名侧维护，Worker 不处理旧域名；确认没有旧客户端后可以关闭该 302。
- `src/main.rs` 的备用清单地址 `ap.launcher-update.nanoda.work` 也依赖旧域名侧的跳转；如果旧域名不再保留，请把它同步改成新地址。
- 回滚：删除 Worker Route（或恢复 SSH 清单上传）。OpenResty `updata/` 中最后一次上传的清单仍然保留，可作为回退。
