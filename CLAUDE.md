# CLAUDE.md

本文件为 Claude Code (claude.ai/code) 在本仓库中工作时提供指引。

**必须使用中文与用户交流。**

## 项目概述

AzurPilot Launcher 是 [AzurPilot](https://github.com/wess09/AzurPilot) 的跨平台（Windows/macOS/Linux）桌面启动器，基于 **Tauri 2 + Rust** 构建。它通过内嵌的 `uv` 二进制文件管理独立的 Python 3.14.6 环境，处理基于 git 的更新，启动 Python WebUI 后端（`gui.py`），并提供原生 webview 壳（含 React 启动首页、系统托盘、通知和自定义标题栏）。

## 构建与开发命令

```bash
npm --prefix frontend ci
cargo tauri dev                         # 自动启动 Vite，支持热更新
npm --prefix frontend test
npm --prefix frontend run build         # tsc + Vite
cargo check --locked --all-targets
cargo test --locked --all-targets        # Windows 需管理员终端
cargo build --locked --release --features custom-protocol
```

Node.js 24 是开发构建依赖；发布后首页无需 Node.js 或 Vite。直接运行 debug Cargo 产物需先执行 `npm --prefix frontend run dev`；生产 Cargo 构建必须先构建前端。`build.rs` 不自动安装 npm 依赖。

构建脚本（`build.rs`）需要 `ALAS_BOOTSTRAP_UV` 环境变量指向要内嵌的 `uv` 二进制文件。本地开发不设置时使用空占位符，启动器会在运行时从 PATH 中查找 `uv` 或通过环境变量 `UV` 指定。

## 启动参数

启动器支持以下命令行参数（Windows 使用 `/` 前缀同样有效）：

| 参数 | 别名 | 说明 |
|---|---|---|
| `--lang <locale>` | `--locale <locale>` | 覆盖系统语言，强制使用指定语言显示界面。支持的值：`zh-CN`（简体中文）、`zh-TW`（繁体中文）、`ja`（日语）、`en`（英语）。也支持 `--lang=zh-CN` 形式。 |
| `--preview-no-update` | `--skip-update`、`--no-update`、`--disable-update` | 跳过仓库更新，直接使用本地文件启动。用于开发调试。 |
| `--preview-crash` | `--preview-error`、`--crash-preview`、`--error-preview` | 模拟启动失败，停留在错误页面以检查 UI 样式。隐含 `--preview-no-update`。 |

示例：

```bash
# 以日语界面启动
AzurPilot --lang ja

# 以英语界面启动，跳过更新
AzurPilot --lang en --skip-update

# 预览错误页面
AzurPilot --preview-crash
```

## 国际化（i18n）

启动器支持 4 种界面语言，根据系统 locale 自动选择，也可通过 `--lang` 启动参数手动覆盖：

- `zh-CN` — 简体中文（基准语言）
- `zh-TW` — 繁体中文
- `ja` — 日语
- `en` — 英语

### 翻译文件

翻译文件位于 `locales/` 目录，使用 YAML 格式：

```
locales/
├── zh-CN.yml    # 简体中文
├── zh-TW.yml    # 繁体中文
├── ja.yml       # 日语
└── en.yml       # 英语
```

每个文件按模块分组（`splash`、`setup`、`dialog`、`tray`、`titlebar`、`error_page`、`notify`、`errors`、`tips`），key 用点分隔。

### 添加新字符串

1. 在 `locales/zh-CN.yml` 中添加 key 和中文值
2. 在其他 3 个语言文件中添加对应翻译
3. 在 Rust 代码中使用 `t!("module.key")` 调用（需要 `.to_string()` 转换为 `String`）
4. 带参数的字符串使用 `t!("module.key", param = value)` 语法，YAML 中用 `%{param}` 占位

### 语言检测逻辑

`src/i18n.rs` 中的 `detect_locale()` 按以下优先级选择语言：
1. 命令行参数 `--lang` / `--locale`（最高优先级）
2. 系统 locale（通过 `sys-locale` crate 获取）
3. 回退到 `en`

支持的 locale 映射：`zh*` → `zh-CN`，`zh-TW`/`zh-HK`/`zh-Hant` → `zh-TW`，`ja*` → `ja`，其他 → `en`。

## 架构

### React 界面与运行时

详细说明见 [启动器界面与生命周期](docs/launcher-ui.md)。

- `frontend/`：React + TypeScript + Vite，CSS Modules，四种语言共用 `locales/*.yml`。
- `launcher.rs`：统一状态、互斥任务、进度事件、托盘与每次后端会话的生命周期。
- `updater.rs`：自动检查与显式安装分离，保留下载校验、mTLS 和平台替换。
- `windows.rs`：WebUI 窗口、导航失败时本地 `#/connection-error` React 页面、标题栏注入。
- `setup.rs`：首次安装、Git 更新与依赖同步；修复只重建 `.venv`，不会更新仓库或清理全局缓存。
- `task_process.rs` / `backend_process.rs`：任务进程组及 Windows Job；`cancel.rs` 让 HTTP 等待可取消。

普通启动先显示首页并检查启动器更新；发现新版后等待用户点击更新。无更新时点击“启动”才执行环境准备、仓库更新和后端启动。运行后首页隐藏（Linux 最小化到任务栏），WebUI 独立显示。首页提供停止服务与环境修复，运行中禁止修复。

所有操作通过 Rust 状态机校验；前端先订阅事件，再请求快照。快照有递增 `sequence` 和独立任务 `task_id`。进度在同一任务内单调，新任务从零开始；依赖子条以子任务标识重置。事件最多 10Hz，状态结束立即推送，React 使用 250ms 线性插值且支持减少动态效果。

关闭启动/修复窗口需确认取消任务并退出，只回收当前任务及所属子进程，不删除环境。后端超时也不自动重建，失败页提供手动重试或修复。新管理命令只允许本地首页调用，连接错误页只允许查询状态、重连、日志与返回首页。

### 自定义标题栏

Windows 和 Linux 移除原生窗口装饰（`set_decorations(false)`），通过 `page_load_injector` 在每个页面加载完成后注入自定义标题栏 JS（红绿灯按钮 + 拖拽区域）。macOS 保留原生标题栏。注入脚本还会：
- 覆盖 `window.saveAs` 使其通过 Tauri 的 `save_as` 命令保存文件
- 阻止浏览器后退（`history.pushState` + `popstate` 监听）

### 窗口关闭行为

- **Windows**：弹出对话框（"退出" / "最小化到托盘"），最小化时 `destroy()` 主窗口释放 WebView 资源，恢复时重新创建
- **macOS**：最小化到托盘，切换 `ActivationPolicy` 为 `Accessory`（隐藏 Dock 图标），恢复时切回 `Regular`
- **Linux**：直接隐藏窗口

### Tauri 前端命令

`launcher_*`（见上文 IPC 文档）、`save_as`、`download_today_gui_log`、`download_today_launcher_log`、`retry_backend_connection`、`window_hide`、`window_minimize`、`window_toggle_maximize`、`window_close`、`window_start_dragging`、`window_is_maximized`

### 端口占用清理

`backend.rs` 在启动 `gui.py` 前会清理占用目标端口的进程：
- **Windows**：解析 `netstat -ano -p tcp` 输出获取 PID
- **Unix**：通过 `lsof -nP -iTCP:{port} -sTCP:LISTEN -t` 获取 PID
- 使用 `sysinfo` 库 kill 进程，等待端口释放（最多 5 秒）

### 子进程泄漏清理

`ManagedBackend` 的 `Drop` 实现会扫描所有进程的环境变量，查找包含 `ALAS_LAUNCHER_PID={当前PID}` 的子进程并 kill，防止 `gui.py` 子进程泄漏。

Windows 后端由 `src/backend_process.rs` 创建允许显式脱离的 Job；普通 worker 仍随 Job 回收，
ALAS 启动模拟器时根据 `ALAS_LAUNCHER_JOB_BREAKAWAY=1` 请求脱离，并清除清理标记和信任密钥。
两边需配套更新，协议、失败处理与隔离验证见 [后端进程生命周期](docs/backend-lifecycle.md)。

### 平台相关代码

大量使用 `#[cfg(target_os = "...")]`。`Cargo.toml` 中的平台依赖：
- **Windows**：`winapi`、`tauri-winrt-notification`、`windows-registry`
- **Linux**：`notify-rust`、`openssl-probe`（CA 证书探测）
- **Unix**：`nix`（信号处理，SIGTERM 优雅退出）
- **桌面端（非移动端）**：`tauri-plugin-single-instance`（单实例，第二次启动时恢复窗口）

### 部署配置

六个 `deploy.*.yaml` 文件配置不同平台/镜像组合。带 `-cn` 后缀的变体使用中国大陆可访问的镜像源。`setup.rs` 中的 `migrate_dependency_config()` 会在启动时自动迁移 `config/deploy.yaml`：
- 更新 Python/Adb/Git 可执行文件路径为 `.venv` 内路径
- 移除已废弃的 `RequirementsFile` 配置项
- 强制设置 `InstallDependencies: true`

### 时间炸弹机制

`Cargo.toml` 中包含 `[package.metadata.azurpilot.time-bomb]` 配置段。`main.rs` 通过 `include_str!("../Cargo.toml")` 在运行时解析此配置（非编译时）。当 `enabled = true` 时，启动器通过 HTTP 请求获取网络时间（`Date` 头），与过期日期比较，过期则弹窗拒绝运行。

### CI/CD

测试流水线（`.github/workflows/test.yml`）：

- 触发条件：推送 `main`、所有 PR 或手动 `workflow_dispatch`
- 测试矩阵：`ubuntu-22.04`、`macos-latest`、`windows-latest`、`windows-11-arm`
- 安装 Linux Tauri 系统依赖及 stable Rust，按 runner/架构缓存 Cargo 依赖；仅 `main` 保存缓存
- 使用 Node.js 24 安装、测试并构建 React 页面，再执行 `cargo check --locked --all-targets` 和 `cargo test --locked --all-targets`，覆盖启动器和示例目标
- 额外的 `worker` 任务用临时清单执行 `bash cf-worker/deploy.sh --dry-run`，校验 Cloudflare Worker 配置保持纯静态（无 `main`）
- 同一事件/分支的新运行取消旧运行；单个矩阵任务失败不取消其他平台，超时为 45 分钟
- 使用 `build.rs` 的 uv/mTLS 空占位符，不需要发布密钥，也不执行打包或部署
- Windows 托管 runner 以管理员身份运行，满足启动器 manifest 和 Node.js 安全目录 ACL 的要求；Windows 本地测试也需管理员终端
- 可在 GitHub Actions 中选择 **Test AzurPilot → Run workflow** 手动运行；本地使用上述两条 Cargo 命令复现

GitHub Actions（`.github/workflows/package.yml`）：
- 触发条件：tag push 或手动 `workflow_dispatch`
- 构建矩阵：`ubuntu-22.04`、`macos-latest`、`windows-latest`
- Linux/macOS 从源码编译 Git v2.49.1，Windows 下载 MinGit v2.51.0
- 下载 Android platform-tools（adb）
- 创建可重定位 `.venv`（Python 3.14.6 + uv + adb + git + requests）
- 打包为 `tar.xz` 归档（国际版 + CN 镜像版）
- 上传版本化启动器更新载荷到 R2 和 CNB Release `1.0.0`；`stable.json` 作为**纯静态资源**部署到 Cloudflare Worker（`cf-worker/`，主地址 `https://launcher-api.azurpilot.com/updata/stable.json`，载荷 URL 为 `https://launcher-download.azurpilot.com/<文件名>`，由该域名 302 到 CNB；无 `main` 脚本，静态请求不消耗 Workers 请求配额；配置与权限见 `cf-worker/README.md`）
- CNB 上传使用 `git-cnb` 1.2.3 和 `CNB_TOKEN` secret；固定 Release `1.0.0`，同名附件先删除再上传以实现覆盖

### 关键常量与配置

- 默认 WebUI 端口：`22267`
- Python 版本：`3.14.6`（`setup.rs` 中 `PYTHON_VERSION`）
- Git 更新最大重试：20 次，间隔 1 秒
- 后端端口等待超时：60 秒
- 后端连接检查超时：500 毫秒
- 通知流断线重连间隔：3 秒
- `UV_PYTHON_INSTALL_MIRROR` 默认使用 npmmirror 加速 Python standalone 下载，并以 python-standalone.org 作为备用源
