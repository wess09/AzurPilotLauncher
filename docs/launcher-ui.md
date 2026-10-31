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

Windows 的 Rust 测试和启动器均需要管理员终端。发布必须先构建前端并启用 `custom-protocol`，否则 debug 配置会使用开发服务器。`build.rs` 不执行 npm 安装。生产字体、视频、JS 和 CSS 均由 Tauri 内嵌，用户运行首页不需要 Node.js；后端 WebUI 在点击启动后由 deploy 脚本构建，构建前启动器会自动检查 Node.js/npm，缺失或过旧时下载经过 SHA-256 校验的便携运行时，无需用户手动安装。

## 用户流程

普通打开先检查启动器更新，检查失败不能启动或修复。发现新版本后等待用户点击更新；下载和校验结束后才开始原子替换并重启。语言与最小化启动参数跨自更新保留。

自更新载荷支持 HTTP Range 时最多使用 8 路连接。遇到 HTTP 429 时，已连接的下载继续、成功分片保留，失败区间重新入队并将后续并发减半，最低降至 1 路；所有新请求共同遵守限流冷却期。重试间隔由 `backon` 生成指数退避和随机抖动（最长 60 秒），同时遵守 `Retry-After` 秒数或 HTTP-date（异常值最多等待一天）。429 持续重试至恢复或用户取消；超时、断流、408 和 5xx 每片最多重试 8 次，再尝试备用源。断流分片从已写入的偏移续传，保留大小、响应区间和最终 SHA-256 校验。Range 探测及单连接请求也遵守相同的 HTTP 重试策略；取消每 100ms 检查一次，不等待退避或网络超时。

缺少引导物时按钮显示“部署”，文件齐备后显示“启动”，已有完整目录不因缺少启动器安装记录而显示部署。“部署”只下载 git/adb 组件及缺失的 deploy 脚本、模板，完成后返回首页，等待用户点击“启动”。点击“启动”才由内嵌 uv 与 deploy 脚本安装 Python、拉取仓库、同步依赖、构建前端并启动后端。成功后显示 WebUI，隐藏首页；Linux 无托盘时将首页最小化到任务栏。托盘可分别打开首页或显示/隐藏 WebUI。`--start-minimized` 仅在引导物齐备时自动启动；缺少引导物、需要更新或发生错误时显示首页。

运行时首页提供“打开 WebUI”和“停止服务”，禁止修复。停止后回收当前后端、工作进程和该会话的 WS 会话，再允许重新启动或修复；已显式脱离后端 Job 的模拟器继续运行。

修复需要确认：绕过已有 ZIP 缓存，重新联网下载并覆盖 git/adb 组件，以及清单中的全部 `deploy/` 脚本和当前系统模板，删除并重建 `.venv`（Python、requests 与依赖重新安装），完成后返回首页。其他代码、用户配置、日志和下载缓存保留。下载 ZIP 保留 SHA-256 校验；普通启动不逐文件校验内容。组件提交失败会回滚；引导文件逐文件原子替换，失败时保留已完成的替换。详见 [部署引导组件](runtime-components.md)。

首页未开始任务时关闭直接退出，运行中关闭只隐藏首页。启动或修复期间关闭需确认“取消任务并退出”，取消操作只回收任务及其子进程，不删除环境、配置或缓存。自更新进入替换交接阶段后不可中断。Windows WebUI 原有“退出 / 最小化到托盘”菜单保持原样。

## 状态与 IPC

`launcher::Launcher` 持有唯一的状态控制器；React 按 `can_*` 显示操作，Rust 同时校验任务槽位、更新门禁及后端状态。阶段为 `checking`、`update_available`、`updating`、`idle`、`deploying`、`starting`、`running`、`stopping`、`repairing`、`cancelling`、`error`。`can_deploy` 与 `can_start` 根据文件就绪状态互斥。

| 接口 | 用途 |
| --- | --- |
| `launcher_context` / `launcher_snapshot` | 当前语言、版本、平台与完整状态 |
| `launcher_check_update` / `launcher_install_update` | 检查更新 / 显式下载安装 |
| `launcher_deploy` / `launcher_start` / `launcher_stop` / `launcher_repair` | 仅部署 / 启动 / 停止 / 修复 |
| `launcher_open_webui` / `launcher_show_home` / `launcher_back` | 打开 WebUI / 返回首页 / 清除可恢复错误 |
| `launcher_cancel_and_exit` | 确认取消并退出 |
| `launcher://state` | 窗口定向的完整快照事件 |
| `launcher://close-requested` | 请求 React 展示退出确认 |

页面先监听状态事件，再请求快照，按全局递增 `sequence` 拒绝乱序或重复消息。每个操作有独立 `task_id`，迟到的更新和完成通知不能修改后续任务。普通进度最多每 100ms 推送一次，完成、错误和取消立即推送。

主进度与依赖子进度保留小数精度，同一任务内不回退；新操作重置主进度，新依赖子任务重置子进度。React 在 250ms 内插值，条形和百分比使用同一个呈现值，隐藏页面和减少动态偏好立即停止动画。

新管理命令通过 `permissions/launcher.toml` 和 `capabilities/*.json` 使用 Tauri 的 IPC 权限机制：按本次调用的来源和窗口标签授权，仅本地首页可修改环境，不再通过再次读取原生 WebView 当前 URL 判定调用来源，避免 macOS URL 查询差异导致误拒。`main` 的本地 `#/connection-error` 页面可读取状态、重连、导出日志及返回首页；远程 WebUI 仅保留窗口与文件桥接命令，范围为 `main` 窗口内的 `http://127.0.0.1:*`，兼容自定义后端端口。连接失败原因保存在 Rust 状态中，不拼进 URL。WebUI 的标题栏和文件保存继续使用已有注入脚本。前端操作被拒绝时显示具体错误，后续状态推送不会清掉错误提示，用户可再次点击重试；设置保存失败时回滚并显示原因。

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

状态快照的 `deployment_required` 由 Rust 检查实际文件是否齐备生成；安装记录缺失时使用缓存清单及已有配置，不以缺少记录本身判定需要部署。设置弹窗提供组件、Python、PyPI 下载源，全部显示镜像名。自动选源读取系统原始语言与 IANA 时区，界面语言覆盖不改变自动判断。

设置弹窗另有“减弱动态效果”与“阻止计算机自动睡眠”两个开关。背景视频持续可见播放会让 WebView2 持有显示器唤醒锁、系统因此无法息屏，“减弱动态效果”暂停背景视频并停用步骤文字与进度填充动画（与系统 `prefers-reduced-motion` 同等对待）；“阻止计算机自动睡眠”由 Rust 侧 `power` 模块持有系统电源锁（Windows `SetThreadExecutionState` 专用线程 / macOS `IOPMAssertion` / Linux `systemd-inhibit` 子进程随宿主退出），只阻止空闲进入睡眠、不阻止息屏，启动器启动时按设置自动恢复。

窗口隐藏到托盘、最小化到任务栏，以及后端启动后自动隐藏首页时，Rust 同时隐藏对应的 WebView，而非只操作父窗口。Windows 下该操作设置 WebView2 `IsVisible=false`，使页面收到 `visibilitychange` 并暂停视频、释放 Video Wake Lock；原生窗口的尺寸与焦点事件覆盖系统最小化和任务栏恢复，页面加载完成时同步初始隐藏状态。恢复窗口后重新显示 WebView，视频仍遵守系统/设置中的减弱动态效果及音频偏好。视频不使用 HTML `autoplay`，由可见性与偏好统一决定是否播放；托盘保留 WebView 时同样生效，且不会暂停后端服务。

设置弹窗还提供“更新通道”与“更新下载源”。“更新通道”选正式版（stable.json 顶层字段）或金丝雀版（顶层 `canary` 字段，每次 main 构建发布，基于 Cargo 版本将 patch 加 1，版本号为 `{major}.{minor}.{patch+1}-canary.{提交哈希前 8 位}.{UTC 时间戳}`）。同 core 版本下，正式版选择金丝雀通道可安装最新金丝雀，金丝雀切回正式通道可安装同号正式版；同通道按版本号及金丝雀时间戳判断更新，core 更低的构建不提示更新。切换通道后立即静默检查一次更新，结果反映到首页主按钮，由用户手动确认安装。“更新下载源”为自动 / 官方源 / GitHub / Cloudflare CDN：载荷按源对 `url`/`github_url`/`cloudflare_url` 排序尝试，自动模式沿用组件源的国内判定（简体中文+北京时间 → 官方源，否则 GitHub 优先、Cloudflare 次之）。启动器每 10 分钟静默轮询一次更新：发现新版本时首页主按钮由“打开 WebUI”变为“更新”，并发一条原生通知（同一版本只提醒一次，点击唤起首页）。
