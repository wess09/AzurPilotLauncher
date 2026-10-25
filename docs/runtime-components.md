# 部署引导组件与 deploy 脚本启动

启动器 exe 零内嵌依赖（仅内嵌 uv 引导二进制），可单独放进空目录运行。首页缺少引导物时显示"部署"，齐备后显示"启动"。部署只下载引导物；Python、源码、依赖与前端全部在启动阶段由内嵌 uv 与后端 deploy 脚本自动完成。

## 唯一的线上清单

主地址为 `https://launcher-api.azurpilot.com/updata/stable.json`，备用地址与启动器自更新相同。编译时的 `LAUNCHER_UPDATE_URL` 同时作用于启动器更新和组件部署。GitHub/CNB Release 仅保存组件 ZIP，不发布第二份运行清单。

`stable.json` 的原有 `version`、`pub_date`、`notes` 和 `platforms` 字段继续用于启动器自更新。顶层 `runtime`（schema 2）：

| 字段 | 含义 |
| --- | --- |
| `schema` | 组件协议版本，当前为 `2` |
| `revision` | 构建批次对应的后端完整 40 位提交号，仅作批次一致性与日志标识，不锁定 deploy 下载 |
| `platforms` | 平台到组件数组的映射，每平台恰好 `git` 与 `adb` 两个组件 |
| 组件 `sha256` | ZIP 的完整 SHA-256，亦用于内容寻址文件名和本地缓存 |
| 组件 `size` | ZIP 精确字节数 |
| 组件 `urls` | 按顺序尝试的 CNB、GitHub ZIP 地址，全部使用 HTTPS |
| `deploy.bases` | 后端仓库 raw 基址（CNB `/-/git/raw/master` 与 GitHub raw `master`），跟随 master 浮动 |
| `deploy.files` | deploy 引导脚本与模板文件清单，由 CI 每轮构建刷新 |

`runtime.platforms` 发布 `windows-x86_64`、`linux-x86_64`、`macos-aarch64`。Windows ARM64 使用完整的 x64 git/ADB，通过系统 x64 兼容能力运行。其他架构若无构建产物，明确报错。

## 两个二进制组件与 deploy 引导清单

| ID | 内容与安装位置 |
| --- | --- |
| `git` | Windows 为最新 MinGit（busybox 64-bit，经 git-for-windows latest release 解析）；macOS/Linux 为发行包布局。安装到 `.azurpilot/tools/git/`，包含对应平台的命令、执行助手和模板 |
| `adb` | Google platform-tools 的 adb 及其 DLL/许可，安装到 `.azurpilot/tools/adb/` |
| `deploy` | 非组件：清单列出 `deploy/**` 部署脚本与 `config/deploy.template*.yaml` 四个分系统模板的 raw 下载路径。启动器按 `DownloadSource` 排序双源逐文件下载（非空校验、原子写入），`config/` 下的 json 等其余文件一律不下载、不修改 |

deploy 引导文件按清单逐项检查，只下载缺失或为空的脚本和当前系统模板。全部就绪后才提交部署记录；中途失败后再次点击会补齐剩余文件。已有非空文件由 git 更新接管，不被引导下载覆盖。模板选择与后端 `get_deploy_template` 一致：Windows 用 `deploy.template.yaml`、macOS/Linux 用 `deploy.template-linux.yaml`，国内网络环境（系统简体中文且北京时间，或显式选择 CNB）自动改用对应 `-cn` 变体。

## 部署与启动流程

部署（首页"部署"按钮触发，只下载引导物）：

1. 读取 stable.json `runtime`，下载/校验/解压 `git`、`adb` 组件并成组提交；
2. 按清单补齐 deploy 脚本与当前系统模板，确认全部就绪后提交组件和部署记录。

部署完成后回到首页，按钮变为“启动”，等待用户再次点击。部署任务不安装 Python、不运行 deploy 脚本，也不拉取仓库、构建前端或启动后端。

启动（首页"启动"按钮触发，完成其余一切）：

1. 内嵌 uv（编译期经 `AZURPILOT_BOOTSTRAP_UV` 嵌入，空占位时回退 `UV` 环境变量或 PATH）安装最新稳定 Python 3.14.x 并创建可重定位 `.venv`；
2. `uv pip install requests`（deploy 脚本的引导依赖，多镜像回退）；
3. 迁移 `config/deploy.yaml`（优先以下载的平台模板为起点，写入 Python、独立工具目录路径与 `InstallDependencies: true`）；
4. 运行 `deploy.git.GitManager` 经 git/GitOverCDN 拉取仓库源码与资源；
5. `uv sync --no-dev --no-install-project` 按 `uv.lock` 同步依赖，多 PyPI 镜像重锁回退；
6. 依赖安装完成后运行 `deploy.app`（`ensure_frontend`）构建前端；无 Node.js 的机器明确报错进入错误页。

启动 gui.py 时清除旧的 `AZURPILOT_PREBUILT` 标记，只设置 `AZURPILOT_NO_UV_BOOTSTRAP=1`。后端使用正常源码模式，不再检查旧六组件协议。Git/ADB 保存在 `.azurpilot/tools/`，与 Python 的 `.venv` 生命周期分开。

环境修复（首页确认后）：优先读取当前线上清单，忽略现有组件和 ZIP 缓存，重新联网下载 git/adb ZIP，经原有 ZIP SHA-256 校验后覆盖安装，再删除并重建 `.venv`（Python + requests + uv sync）。保留代码、配置、日志；下载失败或取消不会预先删除原组件和有效缓存。运行中必须先停止服务，修复完成后等待用户点击启动。

## 校验、取消与恢复

组件 ZIP 下载经 SHA-256 与大小校验，正常部署可复用 `.azurpilot/cache/<sha256>.zip`，显式修复强制重新下载。启动前只检查所需文件是否齐全，不逐文件计算 SHA-256，也不按旧记录的文件长度判断版本变化。文件内容损坏时由用户执行环境修复重新下载覆盖。

`.azurpilot/installed.json` 记录已部署文件，但不是显示“启动”的必需条件。没有安装记录的已有目录，会根据更新检查时缓存的 `runtime.deploy.files` 检查实际脚本，并识别现有模板或 `config/deploy.yaml`、Git/ADB 可执行文件；兼容旧 `.venv` 工具路径及已有配置中的有效路径。首次离线且没有清单缓存时使用启动必需脚本列表。Python、后端源码、资源与构建后的前端不属于“部署”按钮的就绪条件。

解压前拒绝路径穿越、绝对路径、符号链接、Windows 保留名与重复路径；写入前检查既有路径的链接与 reparse point。持久化 `journal.json` 保证提交阶段中断后可回滚恢复。

## CI 与发布顺序

所有打包位于 `.github/workflows/package.yml`：

1. 解析一次后端 `master` 提交作为批次 revision。
2. 三平台各自 checkout 后端，运行 `tools/build_components.py`：打包 git（Windows 经 GitHub API 取最新 MinGit；macOS/Linux 收集 runner 的 git 布局）与 adb（Google platform-tools）为内容寻址 ZIP，生成 deploy 引导文件清单；解压组件到空目录验证 `git --version` 与 `adb version` 可执行。不再运行 npm、uv 或 Python 安装。
3. 合并三平台片段，校验 revision 与 deploy 清单一致、平台完整。
4. 上传 ZIP 到启动器 GitHub `runtime` Release 和 CNB `1.0.0` Release，保留旧摘要包供旧清单下载。
5. 读取线上 stable.json，仅替换其 `runtime` 字段，经现有纯静态 Worker 发布。
6. tag 发布同时构建启动器：CI 先经 `astral-sh/setup-uv` 安装 uv 并以 `AZURPILOT_BOOTSTRAP_UV` 嵌入 exe；启动器更新字段发布时保留最新 `runtime`。

两种清单发布任务共用 `launcher-stable-manifest` 并发锁。组件构建支持手动触发、启动器 `main` 相关变更、外部 `repository_dispatch` 与六小时定时检查；AP 仓库不需要新增通知工作流。

## 本地验证

```powershell
npm --prefix frontend test
npm --prefix frontend run build
uv run --no-project python -m unittest discover -s tools -p "test_*.py"
cargo check --locked --all-targets
cargo test --locked --all-targets
# 显式验证真实 uv 首次安装与修复（会下载 Python 和 requests）
cargo test real_uv_empty_install_and_repair -- --ignored --test-threads=1
```

Windows 的正式 manifest 保持 `requireAdministrator`；组件测试可用 `__COMPAT_LAYER=RunAsInvoker` 以当前权限运行。debug 构建可用 `AZURPILOT_REPO_DIR` 指定空的安装测试目录，用 `AZURPILOT_STABLE_MANIFEST` 指定包含 `runtime` 的本地 HTTP 测试清单；`AZURPILOT_BOOTSTRAP_UV` 指向本地 uv 二进制即可内嵌验证。

## 自动选源与命名设置

自动模式必须同时满足系统原始语言为简体中文、IANA 时区为 `Asia/Shanghai`（含北京时间别名）才优先 CNB；其余优先 GitHub。组件 ZIP 与 deploy raw 下载均按此排序双源回退。设置保存到启动器 SQLite 数据库，支持组件来源、Python 来源和 PyPI 来源；Python 镜像注入 `UV_PYTHON_INSTALL_MIRROR` 首选，PyPI 镜像作为 uv sync 与 requests 安装的首选索引，`config/deploy.yaml` 中的 `PypiMirror` 用户配置仍具最高优先级。
