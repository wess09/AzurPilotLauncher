# 更新日志

## 3.0.3 - 2026-10-26

- 设置新增“减弱动态效果”开关：暂停首页背景视频并停用界面动画，恢复系统正常息屏节奏（背景视频的播放会让 WebView 持显示器唤醒锁导致 Windows 不息屏）。
- 设置新增“阻止计算机自动睡眠”开关：三平台电源锁（Windows `SetThreadExecutionState` / macOS `IOPMAssertion` / Linux `systemd-inhibit`），只阻止空闲睡眠、不影响息屏；启动器重启后自动恢复。
- 新增金丝雀更新通道：每次 main 分支构建自动发布（版本号 `Cargo 版本-canary.{提交哈希}.{UTC 时间戳}`），stable.json 新增顶层 `canary` 字段；设置中可切换正式/金丝雀通道，切换后立即检查更新；同主版本号下正式版恒高于金丝雀，切回正式通道即可平滑覆盖升级。
- 更新载荷新增三源下载：`url`（官方）/ `github_url` / `cloudflare_url`（Cloudflare CDN 国际源）；设置新增“更新下载源”（自动/官方/GitHub/Cloudflare CDN），自动模式沿用国内判定（简体中文+北京时间走官方源，海外 GitHub 优先、Cloudflare 回退）。
- 启动器每 10 分钟静默检查更新：发现新版本时首页主按钮由“打开 WebUI”变为“更新”，并发送一条原生通知（同一版本只提醒一次，点击唤起首页）。
- 修复 macOS 上 IPC 误报“此操作仅允许从启动器本地页面执行”：本地页校验改为仅比较 origin，不再限定路径形式；拒绝时记录窗口实际 URL 便于排查。
- 启动流程新增仓库完整性防呆：git 更新后 `pyproject.toml` 仍缺失时明确提示使用环境修复，不再由 uv sync 抛出晦涩错误。
- macOS 数据目录改为 `AzurPilot.app/Contents/AzurPilot`，旧版目录启动时自动整体迁移；Windows/Linux 便携包目录、tar 产物与 CI 发布命名全面改为 AzurPilot。
- README（四语言）全面更新：当前 UI 单图预览、架构图与目录结构对齐现行实现。
- CI 修复：组件打包访问 GitHub API 携带 `github.token` 规避 runner 共享 IP 匿名限流；更换更新服务 mTLS 客户端证书。

## 3.0.2 - 2026-10-25

- 更新用户提供的 Windows ICO 图标，并用于 Windows 启动器和安装程序。

- 部署回归 deploy 脚本驱动：`stable.json` 的 `runtime` 字段（schema 2）每平台只含 CI 打包的 `git`（Windows 为最新 MinGit，macOS/Linux 为发行包布局）与 `adb` 二进制组件，另附跟随 master 的 deploy 脚本与分系统 `deploy.template*.yaml` 模板的 CNB/GitHub raw 下载清单；`config/` 下的 json 等其余文件一律不下载。
- 部署仅下载上述引导物，缺少时首页显示"部署"；启动阶段由内嵌 uv 安装最新稳定 Python 3.14.x、安装 deploy 引导依赖 requests，再运行 `deploy.git` 拉取仓库源码与资源、uv sync 按 `uv.lock` 同步依赖（多镜像重锁回退），最后由 `deploy.app` 构建前端（无 Node.js 时明确报错）。
- 源码、资源、Python、依赖与前端不再经 CI 构建；运行组件 CI 不再执行 npm 与 uv，构建时长大幅缩短。
- 组件下载保持摘要校验、暂存、成组提交与中断恢复；取消、失败与关闭不删除现有环境。Git/ADB 放在独立工具目录，首次安装和 Python 环境修复不会删除工具；deploy 下载中断后按文件补齐。
- 修正 schema 2 发布校验、源码模式后端启动、Windows 中文路径解码及国内模板/下载源选择。
- 组件、Python、PyPI 下载源设置保留：系统简体中文且使用北京时间时自动优先 CNB，其他环境优先 GitHub；Python/PyPI 镜像在部署与修复时直接生效。

## 3.0.1 - 2026-10-25

- 启动器与 Python 后端改用单条经过本机鉴权的 WebSocket，统一控制命令、原生通知、状态上报和免密登录票据。
- 移除旧 SSE 控制/通知及 HTTP 回报、换票接口；新增心跳、退避重连、命令去重和可取消会话关闭。
- 免密登录使用一次性票据，不向启动器或页面下发后端访问密码。

## 3.0.0 - 2026-10-25

- 全新主界面：三平台统一无边框窗口与自定义标题栏（Logo、版本号、窗口控制按钮、设置入口），内嵌 MiSans 字体并优先于系统字体，界面语言支持在设置中切换（简体中文/繁體中文/日本語/English）并同步启动器自身消息。
- 主面板极简化：空闲与任务执行状态仅保留右下角大按钮，启动/重试按钮内嵌进度填充与当前步骤名（不展示原始 git/uv 输出）；移除状态行、版本行、提示文字与通知条。
- 任务取消重构：新增 `launcher_cancel` 命令，任务执行中点击两次大按钮即可取消且不退出启动器；取消阶段按钮显示"正在取消"。
- 错误报告改为 PCL 风格弹窗：deploy 层错误按命令执行/代码更新/合并/依赖同步分类（结构化错误码经 stderr 标记与进程退出码传递），弹窗按类型展示标题、原始错误日志、"下载错误日志"（整个 log 目录顶层打包为 zip，另存对话框默认定位系统下载文件夹，完成后在文件管理器中定位）与求助提示。
- 新增设置面板（SQLite 存储于系统数据目录）：开机自启（Windows）、启动启动器时直接启动服务、托盘化时保留 WebView 不释放内存（Windows）；非 Windows 平台相关开关自动禁用并标注。
- 错误弹窗按钮不再折行；界面语言、设置项变更即时生效并持久化。

## 2.3.0 - 2026-10-24

- 内嵌 uv 由 0.11.32 升级至 0.13.5：新版证书库移除受泄露影响的 eMudhra/emSign 根证书，并拒绝含被泄露 eMudhra 中间 CA 私钥的证书链，同步更新捆绑 pip/certifi 的证书数据。
- 发行产物统一改名为 AzurPilot：Windows 为 `AzurPilot.exe`，Linux 归档内为 `AzurPilot`，macOS 为 `AzurPilot.app/Contents/MacOS/AzurPilot`。
- Cargo 包名由 `alas-launcher` 改为 `azurpilot`，构建产物为 `target/release/azurpilot[.exe]`；安装脚本、打包脚本、更新载荷名与文档同步更新。
- 应用标识由 `moe.taiho.alas-launcher` 改为 `moe.taiho.azurpilot`，Windows 通知 AUMID 同步改为 `moe.taiho.azurpilot.notification*`。
- 启动器内部函数、JS 全局变量、CSS 类名、DOM 标识和测试环境变量统一使用 `azurpilot` / `AZURPILOT`；构建变量改为 `AZURPILOT_BOOTSTRAP_UV` 和 `AZURPILOT_LAUNCHER_MTLS_IDENTITY_PEM_B64`，CI 兼容读取旧证书 Secret。
- 后端环境协议同时设置 `AP_LAUNCHER_PID`、`AP_LAUNCHER_JOB_BREAKAWAY`、`AP_WEBUI_TRUST_SECRET` 及对应的 `ALAS_` 别名；独立模拟器清除任一归属标记后均不参与兜底清理。已安装的旧版本经自更新升级时只替换文件内容、文件名不变。

## 2.2.5 - 2026-10-19

- 统一启动进度状态，主进度条与 UV 子进度保留历史最高值，修复进度卡顿与回退。
- 启动器自更新清单改由 Cloudflare Worker 纯静态托管，静态资源请求不消耗 Workers 请求配额。
- 自更新清单主地址迁移到 `https://launcher-api.azurpilot.com/updata/stable.json`，旧地址通过 302 跳转。
- 更新启动画面背景视频。

## 2.2.4 - 2026-10-01

- 启动页主进度条新增 loading.webp 角色头部，随进度平滑移动。
- 调整进度条头部的垂直位置，并限制两端位置以避免裁切。

## 2.2.3 - 2026-09-24

- 优化主窗口自定义标题栏胶囊控件在深浅主题下的视觉表现与毛玻璃质感。
- 修复标题栏胶囊控件垂直不居中的问题，动态测量并吸附 WebUI 顶栏高度。
- 优化退出确认弹窗中按钮的交互体验，移除鼠标悬浮时的上浮位移。

## 2.2.1 - 2026-09-19

- 修复主界面顶部拖动区域拦截主页等交互元素点击的问题。
- 统一主窗口与启动页的玻璃质感窗口控制、退出确认层、进度条和提示样式。

## 2.2.0 - 2026-09-17

- 移除启动失败时覆盖整个启动画面的红色遮罩，错误状态沿用启动画面的常规配色。
- 更新启动画面背景视频。

## 2.1.10 - 2026-09-16

- Windows 启动器检测不到系统级 Node.js 时会询问是否自动下载安装；支持 x64、ARM64 与 x86，下载包使用官方 HTTPS 地址和固定 SHA-256 校验。
- Node.js 安装流程使用受保护目录和系统 msiexec.exe，避免管理员启动器的路径或安装包替换风险。

## 2.1.9 - 2026-09-07

- 启动器窗口免密进入 WebUI：启动时生成随机信任密钥并经环境变量注入 WebUI，导航前向本机 WebUI 换取一次性令牌预置登录态，窗口打开即已登录。其他浏览器访问 WebUI 仍需输入密码；手动启动 WebUI 时该免密通道整体关闭。

## 2.1.8 - 2026-07-29

- 完善直连策略：代码更新、内置 Python 下载、UV 与基础依赖安装，以及 WebView 与本地 WebUI 的控制、通知连接均绕过系统代理。
- 更新清单切换至新主地址；主地址不可用、返回异常或清单解析失败时，自动回退到旧地址。升级包改由新的国内 CDN 提供。
- 启动页新增独立的 UV 同步进度，显示依赖解析、下载、安装状态和估算下载速度。
- 后端启动等待延长至 5 分钟；仅在超时后自动重建 `.venv`、重新同步依赖并重试一次。
- 隔离用户全局 Python 与虚拟环境变量，减少本机 Python 配置干扰内置运行环境的问题。
- 启动器日志改为每次启动覆盖当天同名日志，避免多次启动持续堆积。
