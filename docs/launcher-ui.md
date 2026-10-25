# 启动器界面与生命周期

启动器自有页面由 `frontend/` 的 React + TypeScript + Vite 工程实现，Python WebUI 仍由独立的 `main` 窗口加载。首页窗口名为 `launcher`，默认 1100 × 700，最小 900 × 600。

## 开发与构建

开发使用 Node.js 24 和 npm：

```powershell
npm --prefix frontend ci
# 指向安装测试目录（可为空），仅 debug 构建支持覆盖。
$env:AZURPILOT_REPO_DIR = 'C:\path\to\AzurPilot'
cargo tauri dev
```

`cargo tauri dev` 自动运行 Vite；直接运行 `cargo build` 的 debug 产物时，需另开终端执行 `npm --prefix frontend run dev`。开发服务器固定为 `127.0.0.1:1420`，端口被占用时明确失败，不自动换端口。

不启动后端也可单独预览 React。打开 Vite 地址，使用 `?preview=progress`、`repair`、`error`、`running`、`update`、`deploy` 切换模拟状态，使用 `&lang=en`、`ja`、`zh-TW` 检查翻译。模拟模式只在 Vite 开发构建且不存在 Tauri 环境时生效。

```powershell
npm --prefix frontend test
npm --prefix frontend run build
cargo check --locked --all-targets
cargo check --locked --all-targets --features custom-protocol
cargo test --locked --all-targets
cargo build --locked --release --features custom-protocol
```

Windows 的 Rust 测试和启动器均需要管理员终端。发布必须先构建前端并启用 `custom-protocol`，否则 debug 配置会使用开发服务器。`build.rs` 不执行 npm 安装。生产字体、视频、JS 和 CSS 均由 Tauri 内嵌，用户运行首页不需要 Node.js；后端 WebUI 同样由 CI 编译，无运行时 Node.js 安装引导。

## 用户流程

普通打开先检查启动器更新，检查失败不能启动或修复。发现新版本后等待用户点击更新；下载和校验结束后才开始原子替换并重启。语言与最小化启动参数跨自更新保留。

缺少引导物时按钮显示“部署”，齐备后显示“启动”。点击后读取统一 `stable.json` 下载 git/adb 组件并补齐缺失的 deploy 引导脚本，随后由内嵌 uv 与 deploy 脚本安装 Python、拉取仓库、同步依赖后构建前端，最后启动后端。成功后显示 WebUI，隐藏首页；Linux 无托盘时将首页最小化到任务栏。托盘可分别打开首页或显示/隐藏 WebUI。`--start-minimized` 自动启动服务且保持窗口隐藏，需要点击更新或发生错误时显示首页。

运行时首页提供“打开 WebUI”和“停止服务”，禁止修复。停止后回收当前后端、工作进程和该会话的 WS 会话，再允许重新启动或修复；已显式脱离后端 Job 的模拟器继续运行。

修复需要确认：强制重下 git/adb 组件、删除并重建 `.venv`（Python、requests 与依赖重新安装），不更新源码或配置，不清空组件与 uv 缓存，完成后返回首页。部署/修复失败会回滚。详见 [部署引导组件](runtime-components.md)。

首页未开始任务时关闭直接退出，运行中关闭只隐藏首页。启动或修复期间关闭需确认“取消任务并退出”，取消操作只回收任务及其子进程，不删除环境、配置或缓存。自更新进入替换交接阶段后不可中断。Windows WebUI 原有“退出 / 最小化到托盘”菜单保持原样。

## 状态与 IPC

`launcher::Launcher` 持有唯一的状态控制器；React 按 `can_*` 显示操作，Rust 同时校验任务槽位、更新门禁及后端状态。阶段为 `checking`、`update_available`、`updating`、`idle`、`starting`、`running`、`stopping`、`repairing`、`cancelling`、`error`。

| 接口 | 用途 |
| --- | --- |
| `launcher_context` / `launcher_snapshot` | 当前语言、版本、平台与完整状态 |
| `launcher_check_update` / `launcher_install_update` | 检查更新 / 显式下载安装 |
| `launcher_start` / `launcher_stop` / `launcher_repair` | 启动 / 停止 / 修复 |
| `launcher_open_webui` / `launcher_show_home` / `launcher_back` | 打开 WebUI / 返回首页 / 清除可恢复错误 |
| `launcher_cancel_and_exit` | 确认取消并退出 |
| `launcher://state` | 窗口定向的完整快照事件 |
| `launcher://close-requested` | 请求 React 展示退出确认 |

页面先监听状态事件，再请求快照，按全局递增 `sequence` 拒绝乱序或重复消息。每个操作有独立 `task_id`，迟到的更新和完成通知不能修改后续任务。普通进度最多每 100ms 推送一次，完成、错误和取消立即推送。

主进度与依赖子进度保留小数精度，同一任务内不回退；新操作重置主进度，新依赖子任务重置子进度。React 在 250ms 内插值，条形和百分比使用同一个呈现值，隐藏页面和减少动态偏好立即停止动画。

新管理命令同时检查窗口标签和本地页面来源，仅首页可修改环境。`main` 的本地 `#/connection-error` 页面只可读取状态、重连、导出日志及返回首页。连接失败原因保存在 Rust 状态中，不拼进 URL。WebUI 的标题栏和文件保存继续使用已有注入脚本。

四份 `locales/*.yml` 是唯一翻译源。Rust 使用 `t!()`，Vite 构建时读取 `launcher` 分组并校验四种语言键一致，React 使用类型化取词函数处理 `%{parameter}`。

## 回归重点

- 检查更新失败或发现新版时不能绕过更新门禁。
- 启动、修复、停止互斥；停止后再次启动不重复接收通知。
- 取消启动保留 `.venv`；取消修复不会追加清理。
- 安装事务和修复拒绝符号链接或指向工作区外的重解析点；中断后恢复原环境。
- 主动停止不显示连接错误；旧会话的退出/导航回调不能覆盖新会话。
- 关闭 Vite 后，带 `custom-protocol` 的产物仍能显示字体、视频、首页和错误页。

## 后端启动器协议

控制、通知和免密登录统一使用 [启动器 WS v2](launcher-protocol-v2.md)，旧 SSE/HTTP 接口已移除。

状态快照的 `deployment_required` 由 Rust 扫描组件安装记录、git/adb 完整性与 deploy 脚本存在性生成。设置弹窗提供组件、Python、PyPI 下载源，全部显示镜像名。自动选源读取系统原始语言与 IANA 时区，界面语言覆盖不改变自动判断。
