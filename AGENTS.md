# AGENTS.md

本文件为 ZCode 在本仓库中工作时提供指引。**必须使用中文与用户交流。**

详细架构文档见 `CLAUDE.md`（运行时流程、自定义协议、标题栏注入、平台差异的完整说明），本文件只列要点与最新差异。

## 项目概述

AzurPilot Launcher：[AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript) 的跨平台（Windows/macOS/Linux）桌面启动器，**Tauri 2 + Rust**，`frontend/` 使用 React + TypeScript + Vite，生产静态资源由 Tauri 内嵌。独立启动后从同一份 `stable.json` 下载 CI 预构建的源码、资源、前端、最新稳定 Python 3.14.x、依赖与 ADB，再启动 `gui.py`；用户无需 Git、uv 或 Node.js。组件协议见 `docs/runtime-components.md`。

## 常用命令

```bash
npm --prefix frontend ci         # Node.js 24，首次安装前端依赖
npm --prefix frontend test       # React 状态与交互测试
npm --prefix frontend run build  # 前端类型检查与生产构建
cargo build            # debug 构建，运行时需启动 Vite
cargo build --release --features custom-protocol  # 先构建前端，嵌入生产资源
cargo tauri dev        # 开发运行
cargo test             # 全部测试；cargo test <name> 单测
cargo check            # 仅检查编译
```

### 构建脚本环境变量（build.rs）

- `AZURPILOT_LAUNCHER_MTLS_IDENTITY_PEM_B64`：launcher 自更新的 mTLS 客户端证书（base64 PEM）。未设置时回退读取仓库根目录的 `证书.txt`（不入库）。
- `LAUNCHER_UPDATE_URL`：覆盖自更新源，默认 `https://launcher-api.azurpilot.com/updata/stable.json`。

注意：Windows 构建的 manifest 是 `requireAdministrator`（build.rs 内嵌），本地运行会弹 UAC。

## 模块职责

- `frontend/`：React 首页、进度、错误页、确认弹窗；构建时读取四份语言 YAML。
- `src/launcher.rs`：唯一状态控制器、任务互斥、启动/停止/修复/退出、WS 会话和托盘。
- `src/updater.rs`：检查更新、用户显式更新、mTLS/摘要校验、平台替换与参数恢复。
- `src/windows.rs`：窗口恢复、React 连接错误路由、保留的 WebUI 标题栏注入。
- `src/main.rs`：模块入口、日志、信任登录、时间炸弹及通用窗口/文件命令。
- `src/components.rs`：组件清单、按需下载、ZIP 校验、事务提交/恢复及镜像备用安装。
- `src/setup.rs`：定位空的便携目录、配置迁移与用户主动环境修复。
- `src/sources.rs`：自动下载平台选择、Python/PyPI 镜像预设；系统简体中文且北京时间才优先 CNB，否则 GitHub。
- `src/backend.rs` / `backend_process.rs`：后端进程组与 Windows Job，退出不连带关闭独立模拟器。
- `src/backend_env.rs`：同时注入 `AP_` / `ALAS_` 后端环境协议，双归属标记均匹配时才兜底清理子进程。
- `src/task_process.rs` / `cancel.rs`：安装任务进程组与可取消 HTTP，不在取消时删除环境。
- `src/notify.rs` / `launcher_control.rs`：每次后端会话独立的单条 WS 通道、原生通知与停止标志。
- `src/settings.rs`、`autostart.rs`、`i18n.rs`、`window_util.rs`：设置/下载源、自启、多语言和子进程窗口控制。

详细开发流程及 IPC 见 [启动器界面与生命周期](docs/launcher-ui.md)。

## 必须遵守的约定

- **i18n**：4 个语言文件 `locales/{zh-CN,zh-TW,ja,en}.yml`，`zh-CN.yml` 为基准。新增字符串要同时加 4 个文件，Rust 中用 `t!("module.key")`，React 中通过类型化 `translator()` 读取同一 YAML（Rust 常需 `.to_string()`），带参数用 `t!("k", p = v)` + YAML `%{p}`。
- **平台兼容**：改动涉及进程/窗口/通知/路径时必须考虑三平台，用 `#[cfg(target_os = "...")]` 分支；平台依赖在 `Cargo.toml` 的 `[target.'cfg(...)'.dependencies]`。子进程命令一律套 `CreateNoWindow`（Windows）避免黑窗。
- **日志**：用 `tracing`（info!/warn! 等），初始化后写入 `log/{date}_launcher.txt`，不要用 `println!`。
- **版本号**：发布需同步 `Cargo.toml`、`tauri.conf.json`、`CHANGELOG.md` 三处。
- **CI 打包**：仅 `.github/workflows/package.yml`。tag/手动打包轻量启动器；三平台运行组件支持 main 相关变更、后端 dispatch、手动和六小时定时构建。所有线上元数据只写启动器既有 `stable.json` 的 `runtime` 字段；GitHub/CNB 仅存下载包。启动器载荷字段同时写 `url` 与 `github_url`。两类清单发布共用并发锁，合并时必须保留另一部分。沿用纯静态 `cf-worker/` 和现有发布凭据，详见组件协议文档。
- **CI 测试**：`.github/workflows/test.yml` 在推送 `main`、PR 和手动触发时运行，覆盖 Linux、macOS、Windows x64/ARM64，执行 `cargo check --locked --all-targets` 和 `cargo test --locked --all-targets`，无需发布密钥。Windows manifest 要求提权，测试通常在管理员终端运行；纯组件测试也可对当前测试命令设置 `__COMPAT_LAYER=RunAsInvoker`。

## 关键常量

WebUI 端口 `22267`；Python `3.14.x`（CI 解析最新稳定补丁版）；后端端口等待 60s；WS 心跳 15s、重连 1/2/5/10/30s。时间炸弹配置在 `Cargo.toml` 的 `[package.metadata.azurpilot.time-bomb]`（运行时由 `main.rs` 解析，当前 `enabled = false`）。

- **环境修复**：关闭/取消/后端超时均不得自动删除 `.venv` 或缓存。仅首页确认后的修复强制重新部署运行组件，运行中必须先停止服务。
- **前端通信**：新管理命令验证本地 `launcher` 窗口；页面先订阅 `launcher://state` 再取快照，按 `sequence` 去重，按 `task_id` 隔离任务。不要重新引入 Rust HTML 拼接或全局 JS 回调。
