# 独立组件部署

启动器可以单独放进空目录运行。首页缺少组件时显示“部署”，组件齐全时显示“启动”。点击后检查同一份启动器 `stable.json`，下载有变化或缺失的组件，完成部署后启动 `gui.py`。用户无需预装 Git、Python、uv、Node.js 或 npm。

## 唯一的线上清单

主地址为 `https://launcher-api.azurpilot.com/updata/stable.json`，备用地址与启动器自更新相同。编译时的 `LAUNCHER_UPDATE_URL` 同时作用于启动器更新和组件部署。GitHub/CNB Release 仅保存二进制、ZIP 和编译好的 wheel 包，不发布第二份运行清单。

`stable.json` 的原有 `version`、`pub_date`、`notes` 和 `platforms` 字段继续用于启动器自更新。每个平台新增可选 `github_url`，指向 GitHub Release 中已上传的同一份版本化可执行文件；主地址下载或摘要校验失败后尝试 GitHub，取消操作不触发备用下载。旧清单未提供 `github_url` 时仍可读取。

新增顶层 `runtime`：

| 字段 | 含义 |
| --- | --- |
| `schema` | 组件协议版本，当前为 `1` |
| `revision` | 后端完整 40 位 Git 提交号，仅在 CI 解析 |
| `python` | CI 实际安装的稳定 Python `3.14.<patch>` |
| `platforms` | 平台到组件数组的映射 |
| 组件 `id` | 下表中的六个组件之一，每个平台必须完整且无重复 |
| 组件 `sha256` | ZIP 文件的完整 SHA-256，亦用于内容寻址文件名和本地缓存 |
| 组件 `size` | ZIP 精确字节数 |
| 组件 `urls` | 按顺序尝试的 CNB、GitHub ZIP 地址，全部使用 HTTPS |

`runtime.platforms` 发布 `windows-x86_64`、`linux-x86_64`、`macos-aarch64`。Windows ARM64 使用完整的 x64 Python、扩展库和 ADB 组合，通过系统 x64 兼容能力运行，避免混用不同架构的扩展库。其他架构若无构建产物，明确报错。

## 六个独立组件

| ID | 安装位置与内容 |
| --- | --- |
| `source` | 后端源码、配置模板、许可及 `BUILD_MANIFEST`，不含 `.git` 或用户配置 |
| `resources` | `assets/` 游戏资源及 `bin/` 模型、设备端工具，可独立复用 |
| `frontend` | CI 编译的 `frontend/dist/` 和内容指纹 |
| `python` | `.venv/runtime/` 内的可重定位 Python 与标准库，不含第三方依赖 |
| `dependencies` | 同一解释器下按 `uv.lock` 安装的 `site-packages` |
| `adb` | `.venv/Scripts/`（Windows）或 `.venv/bin/`（Unix）内的 ADB、DLL 与许可 |

Windows 的解释器为 `.venv/runtime/python.exe`，Unix 为 `.venv/runtime/bin/python3.14`。这是 Python standalone 安装根目录，不是需要系统解释器的标准 venv。所有子进程禁用用户级 site-packages，并清除外部 `PYTHONHOME`、`PYTHONPATH` 等干扰。

CI 使用最新 uv 的 Python 下载定义，在全新安装目录执行 `uv python install --no-bin 3.14`，选择可获得的最新稳定 3.14 补丁版。精确版本写入 `runtime.python`，后端 Python 约束为 `>=3.14.6,<3.15`。版本选择与依赖构建全部在 CI 完成；用户下载的解释器和扩展库已经过同批次校验。

后端仍有作为业务代码使用的 `deploy` Python 模块和 `config/deploy.yaml`。它们由源码组件提供。启动器本身不再检查 `deploy/installer.py`，正常部署也不调用 `deploy.git`、Git、uv 或 npm。选择镜像或组件源不可用时，按需下载 CI 发布的 uv，使用镜像下载解释器或已经编译的 wheels；不会在用户机器上编译依赖。预构建模式通过 `AZURPILOT_PREBUILT=1` 和 `AZURPILOT_NO_UV_BOOTSTRAP=1` 启用，WebUI 内显示由桌面启动器管理更新，停止服务并再次启动即可取得新组件。

## 校验、取消与恢复

`.azurpilot/installed.json` 记录已部署清单、组件摘要和包内文件大小。启动检查缺失或截断的文件；已完成且相同摘要的组件直接复用。`.azurpilot/cache/<sha256>.zip` 中的完整包经过摘要复核后可再次使用。

部署先下载并校验所有需要的包，解压到 `.azurpilot/transaction/new/`。随后写入持久化 `journal.json`，逐个备份和替换受管理文件，最后提交安装记录。网络失败或下载阶段取消不会修改原环境；提交阶段失败时回滚，进程意外中断后下次部署先恢复。普通关闭、取消和启动失败不删除 `.venv` 或组件缓存。

ZIP 路径必须位于各组件允许的目录内，不接受父目录跳转、绝对路径、符号链接、Windows 特殊路径或重复路径。写入前检查现有路径的符号链接和 reparse point。源码组件不会覆盖用户配置、日志、截图、下载缓存或启动器可执行文件。更新只移除旧安装记录中由组件拥有的废弃文件。

环境修复复用上次完整部署的清单，强制重新部署 Python、第三方依赖和 ADB；下载和校验完成后才应用。代码、资源、用户配置、日志和下载缓存保留。正常启动仍会检查最新清单。

## CI 与发布顺序

所有打包位于 `.github/workflows/package.yml`：

1. 解析一次后端 `master` 提交，三个平台使用同一提交构建。
2. CI 构建后端前端，解析 Python 3.14 补丁版，安装锁定依赖，打六类内容寻址 ZIP；在另一目录重新组装并运行解释器、核心库导入和前端指纹检查。
3. 合并三平台内部清单片段，拒绝源码提交或 Python 补丁号不一致的批次。
4. 上传 ZIP 到启动器 GitHub `runtime` Release 和 CNB `1.0.0` Release，保留旧摘要包以供旧版本修复。
5. 读取线上 `stable.json`，仅替换其 `runtime` 字段，保留启动器版本、各平台主地址和 `github_url`，通过现有纯静态 Worker 发布。
6. tag 发布另外上传启动器可执行文件到 CNB、R2 和对应 GitHub tag Release；在组件发布成功后更新启动器字段，同时保留最新 `runtime`。

两种清单发布任务共用 `launcher-stable-manifest` 并发锁，读取、合并、发布处于同一个任务内，避免互相覆盖。读取原清单失败或产物验证失败时不发布新清单。

组件构建支持手动触发、启动器 `main` 相关变更、后端 `repository_dispatch`，并每六小时检查一次。后端 `.github/workflows/launcher-components.yml` 可用 `LAUNCHER_DISPATCH_TOKEN` 立即通知启动器仓库；令牌需要对 `wess09/AzurPilotLauncher` 的 repository dispatch 写权限。未设置令牌时由六小时检查兜底。沿用启动器现有 CNB、Cloudflare 和 R2 发布密钥。

## 本地验证

```powershell
npm --prefix frontend test
npm --prefix frontend run build
uv run --no-project python -m unittest discover -s tools -p "test_*.py"
cargo check --locked --all-targets
cargo test --locked --all-targets
```

Windows 的正式 manifest 保持 `requireAdministrator`；通常在管理员终端运行 Rust 测试。组件测试本身不需要管理员权限，也可只为当前测试命令设置 `__COMPAT_LAYER=RunAsInvoker`。该设置不改变发布可执行文件的 manifest。

debug 构建可用 `AZURPILOT_REPO_DIR` 指定空的安装测试目录，用 `AZURPILOT_STABLE_MANIFEST` 指定包含 `runtime` 的本地 HTTP 测试清单。release 不读取这些调试覆盖项。

## 自动选源与命名设置

自动模式必须同时满足系统原始语言为简体中文、IANA 时区为 `Asia/Shanghai`（含北京时间别名）才优先 CNB；其余优先 GitHub。UTC+8 的台北、新加坡时区不会被误认成北京时间，修改界面语言也不会改变系统判定。组件和启动器更新均遵循该规则，失败后保留另一个发布平台作为备用。

设置保存到启动器 SQLite 数据库，支持组件来源、Python 来源和 PyPI 来源，旧库缺少这些键时默认为自动。界面只显示镜像名：Python 包含南京大学、中科大、淘宝镜像、阿里云、CNB、Astral；PyPI 包含官方、清华大学、中科大、阿里云、腾讯云、华为云、南京大学。更改来源在下次需要安装或修复时生效，完整组件不因选源变化而重复下载。

显式选择 Python/PyPI 镜像时，备用安装同样先在事务目录准备。`runtime.installers` 提供各平台 uv 包的 URL、大小和摘要，它不是首页部署完整性的必需组件，仅在需要时下载。依赖组件携带实际平台的精确版本清单 `requirements`。CI 与镜像安装沿用后端的 uv 锁定同步行为；默认部署直接下载 CI 打包好的依赖组件，用户无需运行包管理器。失败不会修改现有环境。
