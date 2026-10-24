<div align="center">

<img src="icons/icon.png" alt="AzurPilot Launcher Logo" width="200">

# AzurPilot Launcher

**全平台原生运行 · 内置 Python 环境零配置 · 一键更新启动 AzurPilot**

全功能《碧蓝航线》自动化助手 [AzurPilot](https://github.com/wess09/AzurPilot)（基于 [AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript)）的跨平台桌面启动器，基于 Tauri 2 + Rust 构建，支持 Windows / macOS / Linux。

<p align="center">
  <b>简体中文</b> |
  <a href="README_en.md">English</a> |
  <a href="README_ja.md">日本語</a> |
  <a href="README_zh-TW.md">繁體中文</a>
</p>

---

<!-- 核心环境与平台标签组 -->
<p align="center">
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue.svg?style=flat-square" alt="License: GPL-3.0"></a>
  <a href="https://www.tauri.app/"><img src="https://img.shields.io/badge/Framework-Tauri%202-24C8DB.svg?style=flat-square&logo=tauri&logoColor=white" alt="Framework: Tauri 2"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Language-Rust-DEA584.svg?style=flat-square&logo=rust&logoColor=white" alt="Language: Rust"></a>
  <a href="https://www.python.org/"><img src="https://img.shields.io/badge/Python-3.14.6-3776AB.svg?style=flat-square&logo=python&logoColor=white" alt="Python 3.14.6"></a>
  <a href="https://github.com/astral-sh/uv"><img src="https://img.shields.io/badge/Packaging-uv-DE5FE9.svg?style=flat-square" alt="Packaging: uv"></a>
  <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-181717.svg?style=flat-square&logo=github&logoColor=white" alt="Platform: Windows / macOS / Linux">
</p>

<!-- 技术栈与框架标签组 -->
<p align="center">
  <a href="https://developer.mozilla.org/zh-CN/docs/Web/API/Web_Workers_API"><img src="https://img.shields.io/badge/Shell-WebView%20%2B%20Rust%20Backend-24C8DB.svg?style=flat-square" alt="Shell: WebView + Rust"></a>
  <img src="https://img.shields.io/badge/i18n-4%20Languages-blueviolet.svg?style=flat-square" alt="i18n: 4 Languages">
  <img src="https://img.shields.io/badge/Update-Git%20%2B%20uv%20Sync-brightgreen.svg?style=flat-square" alt="Update: Git + uv Sync">
  <img src="https://img.shields.io/badge/Self%20Update-mTLS%20Secured-orange.svg?style=flat-square" alt="Self Update: mTLS">
  <a href="https://deepwiki.com/wess09/AzurPilotLauncher"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki"></a>
</p>

<!-- 仓库动态与社区指标标签组 -->
<p align="center">
  <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/github/v/release/wess09/AzurPilotLauncher?style=flat-square&color=007ec6&label=Latest%20Release" alt="Latest Release"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/releases"><img src="https://img.shields.io/github/downloads/wess09/AzurPilotLauncher/total?style=flat-square&color=28a745&label=Downloads" alt="Total Downloads"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/stargazers"><img src="https://img.shields.io/github/stars/wess09/AzurPilotLauncher?style=flat-square&color=f5a623&label=Stars" alt="Stars"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/network/members"><img src="https://img.shields.io/github/forks/wess09/AzurPilotLauncher?style=flat-square&color=6f42c1&label=Forks" alt="Forks"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/issues"><img src="https://img.shields.io/github/issues/wess09/AzurPilotLauncher?style=flat-square&color=d73a49&label=Issues" alt="Open Issues"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/commits/main"><img src="https://img.shields.io/github/last-commit/wess09/AzurPilotLauncher?style=flat-square&color=586069&label=Last%20Commit" alt="Last Commit"></a>
</p>

<p align="center">
  <a href="#项目总览与指标">项目总览</a> •
  <a href="#项目概述">项目概述</a> •
  <a href="#关联生态工程">关联生态</a> •
  <a href="#核心特性">核心特性</a> •
  <a href="#系统架构全景">系统架构</a> •
  <a href="#环境要求">环境要求</a> •
  <a href="#快速开始">快速开始</a> •
  <a href="#应用预览">应用预览</a> •
  <a href="#与原版启动器的区别">版本区别</a> •
  <a href="#目录结构">目录结构</a> •
  <a href="#研发活跃度">研发活跃</a> •
  <a href="#star-增长趋势">增长趋势</a> •
  <a href="#社区生态与支持">社区支持</a>
</p>

</div>

---

## 项目总览与指标

<table width="100%">
  <thead>
    <tr>
      <th colspan="4" align="left">
        <img src="https://img.shields.io/badge/Project%20Overview-AzurPilot%20Launcher-181717?style=flat-square&logo=github&logoColor=white" alt="Overview">
        <b>工程核心运行与研发指标</b>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="25%"><b>最新正式版</b><br><a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/github/v/release/wess09/AzurPilotLauncher?style=flat-square&color=007ec6" alt="Release"></a></td>
      <td width="25%"><b>累计下载量</b><br><a href="https://github.com/wess09/AzurPilotLauncher/releases"><img src="https://img.shields.io/github/downloads/wess09/AzurPilotLauncher/total?style=flat-square&color=28a745" alt="Downloads"></a></td>
      <td width="25%"><b>开源许可证</b><br><a href="./LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"></a></td>
      <td width="25%"><b>自动化构建</b><br><a href="https://github.com/wess09/AzurPilotLauncher/actions"><img src="https://img.shields.io/badge/CI-GitHub%20Actions-2088FF?style=flat-square&logo=githubactions&logoColor=white" alt="CI Status"></a></td>
    </tr>
    <tr>
      <td width="25%"><b>代码库体积</b><br><img src="https://img.shields.io/github/repo-size/wess09/AzurPilotLauncher?style=flat-square&color=586069" alt="Repo Size"></td>
      <td width="25%"><b>代码语言</b><br><img src="https://img.shields.io/badge/Language-Rust%20%7C%20Tauri-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Language"></td>
      <td width="25%"><b>运行时</b><br><img src="https://img.shields.io/badge/Python-3.14.6%20%2B%20uv-3776AB?style=flat-square&logo=python&logoColor=white" alt="Python Runtime"></td>
      <td width="25%"><b>界面语言</b><br><img src="https://img.shields.io/badge/i18n-%E7%AE%80%E4%BD%93%20%7C%20%E7%B9%81%E9%AB%94%20%7C%20%E6%97%A5%E6%9C%AC%E8%AA%9E%20%7C%20En-blueviolet?style=flat-square" alt="i18n"></td>
    </tr>
    <tr>
      <td colspan="4">
        <b>快速操作快捷入口：</b>
        <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/badge/Release-下载最新压缩包-0052cc?style=flat-square&logo=github&logoColor=white" alt="Download"></a>
        <a href="https://deepwiki.com/wess09/AzurPilotLauncher"><img src="https://img.shields.io/badge/Wiki-Ask%20DeepWiki-6366F1?style=flat-square" alt="Ask DeepWiki"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/issues/new/choose"><img src="https://img.shields.io/badge/Issue-提交错误报告与需求-d73a49?style=flat-square&logo=githubissues&logoColor=white" alt="New Issue"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/pulls"><img src="https://img.shields.io/badge/PR-合并请求代码贡献-28a745?style=flat-square&logo=git&logoColor=white" alt="Pull Request"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/stargazers"><img src="https://img.shields.io/badge/Star-关注项目发展-f5a623?style=flat-square&logo=github&logoColor=white" alt="Star"></a>
      </td>
    </tr>
  </tbody>
</table>

---

## 项目概述

**AzurPilot Launcher** 致力于让全功能《碧蓝航线》自动化助手 **AzurPilot** 在 Windows、macOS 与 Linux 上以原生方式一键运行——无论是 Apple Silicon 的 Mac Mini，还是传统 x86 主机，都无需转译、无需 Docker、不污染系统环境。

启动器内嵌 `uv` 包管理器，首次启动自动创建可重定位的 `.venv`（Python 3.14.6），经 git 拉取最新代码并同步依赖后，启动 Python WebUI 后端，并以原生 WebView 壳承载——内置启动画面、系统托盘、桌面通知与自定义标题栏。界面支持简体中文、繁体中文、日语、英语四种语言，按系统环境自动选择。

---

## 关联生态工程

本工程与相关联的核心项目紧密联动，关键依赖与来源项目如下：

<table width="100%">
  <tr>
    <td width="33%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher">
          <img src="https://img.shields.io/badge/Repository-AzurPilot%20Launcher-181717?style=for-the-badge&logo=github&logoColor=white" alt="AzurPilot Launcher">
        </a>
      </div>
      <br>
      <b>跨平台启动器 (当前项目)</b>
      <p>面向 Windows / macOS / Linux 的一体化桌面启动器，打通环境搭建、代码更新、依赖同步与 WebUI 生命周期管理。</p>
      <hr>
      <div>
        <img src="https://img.shields.io/badge/Language-Rust-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Rust">
        <img src="https://img.shields.io/badge/Framework-Tauri%202-24C8DB?style=flat-square" alt="Tauri 2">
        <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0">
      </div>
    </td>
    <td width="33%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilot">
          <img src="https://img.shields.io/badge/Repository-AzurPilot-181717?style=for-the-badge&logo=github&logoColor=white" alt="AzurPilot">
        </a>
      </div>
      <br>
      <b>核心自动化引擎本体</b>
      <p>《碧蓝航线》自动化助手核心，包含完备的计算机视觉图像识别、出击调度算法与本地 Web 控制台服务。</p>
      <hr>
      <div>
        <img src="https://img.shields.io/badge/Language-Python%203.14-3776AB?style=flat-square&logo=python&logoColor=white" alt="Python">
        <img src="https://img.shields.io/badge/WebUI-React-61DAFB?style=flat-square&logo=react&logoColor=black" alt="React">
        <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0">
      </div>
    </td>
    <td width="33%" valign="top">
      <div align="center">
        <a href="https://github.com/LmeSzinc/AzurLaneAutoScript">
          <img src="https://img.shields.io/badge/Repository-AzurLaneAutoScript-181717?style=for-the-badge&logo=github&logoColor=white" alt="AzurLaneAutoScript">
        </a>
      </div>
      <br>
      <b>上游自动化脚本 (AzurLaneAutoScript)</b>
      <p>AzurLaneAutoScript 上游项目，碧蓝航线自动化的奠基之作，本生态链的全部自动化能力由此演化而来。</p>
      <hr>
      <div>
        <img src="https://img.shields.io/badge/Core-AzurLaneAutoScript%20Framework-0052cc?style=flat-square" alt="AzurLaneAutoScript">
        <img src="https://img.shields.io/badge/Vision-OpenCV%20%7C%20OCR-5C3EE8?style=flat-square" alt="Vision">
        <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0">
      </div>
    </td>
  </tr>
</table>

---

## 核心特性

<table width="100%">
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模块-部署形态-0052cc?style=flat-square" alt="Tag"><br>
      <h3>零配置环境搭建</h3>
      <p>启动器内嵌 <code>uv</code>，首次运行自动下载 Python 3.14.6 并创建<b>可重定位 <code>.venv</code></b>，同时将 uv、git、adb 一并部署进虚拟环境。用户机器无需预装 Python、uv 或 Git，开箱即跑。</p>
      <div>
        <img src="https://img.shields.io/badge/运行时-Python%203.14.6-orange?style=flat-square" alt="Python">
        <img src="https://img.shields.io/badge/依赖管理-uv%20内置-blueviolet?style=flat-square" alt="uv">
        <img src="https://img.shields.io/badge/体验-开箱即跑-brightgreen?style=flat-square" alt="Ready">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模块-原生体验-00875a?style=flat-square" alt="Tag"><br>
      <h3>原生桌面壳层体验</h3>
      <p>基于 Tauri 2 的原生 WebView 壳：玻璃质感启动画面与自定义标题栏、系统托盘、Windows Toast / macOS / Linux 原生通知、单实例运行，重复启动自动聚焦已有窗口。</p>
      <div>
        <img src="https://img.shields.io/badge/框架-Tauri%202-blue?style=flat-square" alt="Tauri">
        <img src="https://img.shields.io/badge/交互-托盘%20%2B%20通知%20%2B%20标题栏-teal?style=flat-square" alt="Native">
        <img src="https://img.shields.io/badge/实例-单实例-lightgrey?style=flat-square" alt="Single Instance">
      </div>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模块-更新体系-403294?style=flat-square" alt="Tag"><br>
      <h3>自动化更新体系</h3>
      <p>启动时经 git 自动拉取最新代码（最多重试 20 次），再用 <code>uv sync --frozen</code> 按 <code>uv.lock</code> 精确同步依赖；启动器本体经 mTLS 安全通道自更新，支持多镜像回退。</p>
      <div>
        <img src="https://img.shields.io/badge/代码更新-Git%20重试机制-purple?style=flat-square" alt="Git Update">
        <img src="https://img.shields.io/badge/依赖锁定-uv.lock-blue?style=flat-square" alt="Lock">
        <img src="https://img.shields.io/badge/自更新-mTLS%20多镜像-orange?style=flat-square" alt="mTLS">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模块-国际化-172b4d?style=flat-square" alt="Tag"><br>
      <h3>四语言国际化界面</h3>
      <p>内置简体中文、繁体中文、日本語、English 四种界面语言，按系统 locale 自动选择，亦可通过 <code>--lang</code> 启动参数强制指定。WebUI 与 Web 通知按所选语言本地化呈现。</p>
      <div>
        <img src="https://img.shields.io/badge/语言-简体中文-red?style=flat-square" alt="zh-CN">
        <img src="https://img.shields.io/badge/語言-繁體中文-blue?style=flat-square" alt="zh-TW">
        <img src="https://img.shields.io/badge/言語-日本語-pink?style=flat-square" alt="ja">
        <img src="https://img.shields.io/badge/Language-English-green?style=flat-square" alt="en">
      </div>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模块-网络策略-de350b?style=flat-square" alt="Tag"><br>
      <h3>双镜像与直连策略</h3>
      <p>提供国际版与 <code>-cn</code> 国内镜像版两种部署配置；Python standalone 下载默认走 npmmirror 加速。更新代码、内置 Python、uv 与依赖安装，以及 WebView 与本地 WebUI 的连接均绕过系统代理，减少环境干扰。</p>
      <div>
        <img src="https://img.shields.io/badge/部署-国际版%20%2F%20CN%20镜像-red?style=flat-square" alt="Mirrors">
        <img src="https://img.shields.io/badge/加速-npmmirror-red?style=flat-square" alt="npmmirror">
        <img src="https://img.shields.io/badge/连接-智能直连-green?style=flat-square" alt="Direct">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模块-生命周期-ff5630?style=flat-square" alt="Tag"><br>
      <h3>稳健的进程与窗口管理</h3>
      <p>启动后端前自动清理端口占用进程；经环境变量标记子进程归属，退出时全进程表扫描回收，杜绝 <code>gui.py</code> 泄漏；启动 WebUI 免密直达（一次性令牌预置登录态），窗口打开即已登录。</p>
      <div>
        <img src="https://img.shields.io/badge/端口-占用自动清理-blue?style=flat-square" alt="Port Clean">
        <img src="https://img.shields.io/badge/进程-零泄漏-brightgreen?style=flat-square" alt="Leak Free">
        <img src="https://img.shields.io/badge/WebUI-免密直达-009688?style=flat-square" alt="Auth Free">
      </div>
    </td>
  </tr>
</table>

---

## 系统架构全景

项目采用清晰的分层解耦设计，各层间职责清晰、通讯边界规范：

```mermaid
graph TD
    subgraph Shell ["启动器壳层 (Tauri 2 · Rust)"]
        Splash["启动画面 React launcher<br/>(玻璃质感进度条 / 错误页 React connection error)"]
        Setup["环境搭建 setup.rs<br/>(Python / uv / adb / git 安装与迁移)"]
        Tray["系统集成<br/>(系统托盘 / 原生通知 / 自定义标题栏)"]
        Backend["后端托管 backend.rs<br/>(端口清理 / 进程回收)"]
    end

    subgraph Venv [".venv 运行时 (uv 管理 · 可重定位)"]
        PYTHON["CPython 3.14.6 解释器"]
        TOOLS["uv / git / adb 内置工具链"]
        SYNC["uv sync --frozen 依赖同步 (uv.lock 锁定)"]
    end

    subgraph Repo ["AzurLaneAutoScript 仓库"]
        GIT["Git 自动更新 (deploy.git.GitManager · 重试 20 次)"]
        GUI["gui.py WebUI 后端"]
        WEB["WebUI 控制台 (127.0.0.1:22267)"]
    end

    Splash --> Setup
    Setup --> TOOLS
    TOOLS --> PYTHON
    Setup --> GIT
    GIT --> SYNC
    SYNC --> GUI
    Backend --> GUI
    GUI --> WEB
    Shell -. "WebView 承载 · SSE 通知流" .-> WEB
    Tray -. "桌面通知 / 托盘控制" .-> Shell
```

---

## 环境要求

<table width="100%">
  <thead>
    <tr>
      <th width="20%">维度</th>
      <th width="40%">准入指标</th>
      <th width="40%">说明</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td><b>操作系统</b></td>
      <td><img src="https://img.shields.io/badge/Windows-10%2B%20(x64%20%2F%20ARM64)-0078D6?style=flat-square&logo=windows&logoColor=white" alt="Windows"> <img src="https://img.shields.io/badge/macOS-原生支持-000000?style=flat-square&logo=apple&logoColor=white" alt="macOS"> <img src="https://img.shields.io/badge/Linux-x86__64%20%2F%20ARM64-FCC624?style=flat-square&logo=linux&logoColor=black" alt="Linux"></td>
      <td>CI 以 Ubuntu 22.04 / macOS / Windows 最新版矩阵构建</td>
    </tr>
    <tr>
      <td><b>WebView 运行时</b></td>
      <td><img src="https://img.shields.io/badge/Windows-WebView2-0078D6?style=flat-square" alt="WebView2"> <img src="https://img.shields.io/badge/Linux-libwebkit2gtk--4.1-FCC624?style=flat-square" alt="libwebkit2gtk"> <img src="https://img.shields.io/badge/macOS-系统内置-000000?style=flat-square" alt="WKWebView"></td>
      <td>Windows 7/8/10 需先安装 <a href="https://developer.microsoft.com/zh-cn/microsoft-edge/webview2">WebView2</a>；Linux 需较新 glibc</td>
    </tr>
    <tr>
      <td><b>Python / uv / Git / adb</b></td>
      <td><img src="https://img.shields.io/badge/依赖-无需预装-brightgreen?style=flat-square" alt="No Preinstall"></td>
      <td>启动器内嵌 uv 并自动创建 <code>.venv</code>，自动部署 git 与 adb 至虚拟环境</td>
    </tr>
    <tr>
      <td><b>网络连接</b></td>
      <td><img src="https://img.shields.io/badge/首次启动-需联网-yellow?style=flat-square" alt="Network"></td>
      <td>下载 Python、同步依赖与更新仓库；CN 版走国内镜像加速</td>
    </tr>
    <tr>
      <td><b>Node.js (仅 Windows)</b></td>
      <td><img src="https://img.shields.io/badge/检测-自动处理-orange?style=flat-square&logo=nodedotjs&logoColor=white" alt="Node.js"></td>
      <td>未检测到系统级 Node.js 时，确认后自动下载安装（SHA-256 校验）</td>
    </tr>
  </tbody>
</table>

---

## 快速开始

<table width="100%">
  <tr>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/步骤-01-blue?style=flat-square" alt="Step 1"><br>
        <h4>获取压缩包</h4>
      </div>
      前往 <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest">Releases 最新版本</a> 下载<b>对应系统与 CPU 的压缩包</b>（国际版或 CN 镜像版）。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/步骤-02-indigo?style=flat-square" alt="Step 2"><br>
        <h4>解压并启动</h4>
      </div>
      Windows 运行 <code>AzurPilot.exe</code>（需管理员权限）；macOS 打开 <code>AzurPilot.app</code>；Linux 运行 <code>AzurPilot</code>。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/步骤-03-purple?style=flat-square" alt="Step 3"><br>
        <h4>等待环境初始化</h4>
      </div>
      启动画面实时展示进度：创建 <code>.venv</code>、更新仓库、同步依赖。首次启动需联网，之后重复启动只会聚焦已有窗口。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/步骤-04-green?style=flat-square" alt="Step 4"><br>
        <h4>进入 WebUI</h4>
      </div>
      窗口打开即已登录 AzurPilot WebUI，直接配置出击任务与调度规则，开始自动化巡航。
    </td>
  </tr>
</table>

> [!TIP]
> 启动器支持 `--lang` 参数强制指定界面语言（`zh-CN` / `zh-TW` / `ja` / `en`），以及 `--skip-update` 跳过仓库更新、`--preview-crash` 预览错误页面等调试参数。

> [!WARNING]
> **macOS 用户**：程序未经开发者签名，首次打开若报错，请先在终端运行 `xattr -dr com.apple.quarantine AzurPilot.app` 再启动。

> [!IMPORTANT]
> **Linux 用户**：程序依赖 `libwebkit2gtk-4.1` 和较新的 `glibc`（CI 使用 Ubuntu 22.04 构建）。若系统缺失相关库导致启动器无法运行，AzurPilot 本体通常仍可通过命令行正常使用。

---

## 应用预览

| 平台 | 简体中文 | English |
|:---:|:---:|:---:|
| **Windows** | <img src="screenshots/win-cn.webp" width="420"/> | <img src="screenshots/win-en.webp" width="420"/> |
| **macOS** | <img src="screenshots/mac-cn.webp" width="420"/> | <img src="screenshots/mac-en.webp" width="420"/> |

---

## 与原版启动器的区别

相较于 [AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript) 自带的 Electron 启动器：

| # | 差异点 | 说明 |
|:---:|:---|:---|
| 1 | **全平台支持** | 不再局限于 Windows，macOS（含 Apple Silicon 原生）与 Linux 均可运行 |
| 2 | **更新策略重构** | 只更新 git 仓库，并用 `.venv` 内置的 uv 同步依赖；重复启动仅重新聚焦已有窗口，不做破坏性清理 |
| 3 | **依赖版本锁定** | Python 包版本由 `pyproject.toml` 与 `uv.lock` 锁定，自动同步默认启用，杜绝环境漂移 |
| 4 | **adb 策略** | 不再重启和替换 adb |
| 5 | **目录结构调整** | Python / uv / git / adb 全部收纳进 `.venv`，详见下方目录结构 |
| 6 | **Node.js 自动化 (Windows)** | 检测系统级 Node.js；未检测到时可在确认后自动下载并安装 |

---

## 目录结构

```
AzurPilot 根目录
* Windows: AzurLaneAutoScript
* macOS:   AzurPilot.app/Contents/AzurLaneAutoScript
* Linux:   AzurLaneAutoScript

AzurPilot 启动器
* Windows: AzurLaneAutoScript/AzurPilot.exe
* macOS:   AzurPilot.app/Contents/MacOS/AzurPilot
* Linux:   AzurLaneAutoScript/AzurPilot

Python / uv
* 所有系统: .venv

Git
* Unix:    .venv/bin/git
* Windows: .venv/Scripts/git/cmd/git.exe

Adb
* Unix:    .venv/bin/adb
* Windows: .venv/Scripts/adb.exe

启动器会附加的环境变量
* Unix:    .venv/bin
* Windows: .venv/Scripts、.venv/Scripts/git/cmd
```

---

## 研发活跃度

<table width="100%">
  <thead>
    <tr>
      <th colspan="3" align="left">
        <img src="https://img.shields.io/badge/Metrics-Repo%20Activity-5C3EE8?style=flat-square&logo=github&logoColor=white" alt="Metrics">
        <b>仓库研发活跃度与响应态势</b>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="33%">
        <b>代码提交频度</b><br>
        <img src="https://img.shields.io/github/commit-activity/m/wess09/AzurPilotLauncher?style=flat-square&color=00d4aa" alt="Commit Activity"><br>
        <small>月度提交活跃状态</small>
      </td>
      <td width="33%">
        <b>合并请求处理</b><br>
        <img src="https://img.shields.io/github/issues-pr-closed/wess09/AzurPilotLauncher?style=flat-square&color=6f42c1" alt="Closed PRs"><br>
        <small>已归档合并请求总计</small>
      </td>
      <td width="33%">
        <b>问题反馈解决</b><br>
        <img src="https://img.shields.io/github/issues-closed/wess09/AzurPilotLauncher?style=flat-square&color=28a745" alt="Closed Issues"><br>
        <small>已成功解决 Issue 统计</small>
      </td>
    </tr>
    <tr>
      <td width="33%">
        <b>研发分支</b><br>
        <img src="https://img.shields.io/badge/Branch-main-181717?style=flat-square&logo=git&logoColor=white" alt="Main Branch"><br>
        <small>主干持续交付流</small>
      </td>
      <td width="33%">
        <b>自动化构建</b><br>
        <img src="https://img.shields.io/badge/CI%2FCD-GitHub%20Actions-2088FF?style=flat-square&logo=githubactions&logoColor=white" alt="CI"><br>
        <small>三平台矩阵持续验证</small>
      </td>
      <td width="33%">
        <b>最新提交记录</b><br>
        <a href="https://github.com/wess09/AzurPilotLauncher/commits/main"><img src="https://img.shields.io/github/last-commit/wess09/AzurPilotLauncher?style=flat-square&color=586069" alt="Last Commit"></a><br>
        <small>追踪最新代码演进</small>
      </td>
    </tr>
  </tbody>
</table>

---

## Star 增长趋势

采用自适应深浅模式的 Star-History 矢量历史图谱，直观反映项目发展脉络：

<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date" />
    <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date" width="100%" />
  </picture>
  <br>
  <sub>数据源基于 <a href="https://star-history.com/#wess09/AzurPilotLauncher&Date">Star-History</a> 实时更新 · 点击可查看交互式完整走势</sub>
</div>

---

## 技术栈与依赖致谢

### 启动器技术栈

| 依赖组件 | 许可证标识 | 职责定位 |
| :--- | :--- | :--- |
| **Tauri 2** | <img src="https://img.shields.io/badge/License-MIT%20%7C%20Apache--2.0-brightgreen?style=flat-square" alt="MIT / Apache-2.0"> | 跨平台桌面壳、WebView 承载与系统集成 |
| **Rust** | <img src="https://img.shields.io/badge/License-MIT%20%7C%20Apache--2.0-brightgreen?style=flat-square" alt="MIT / Apache-2.0"> | 系统编程语言，启动器全部原生逻辑 |
| **uv** | <img src="https://img.shields.io/badge/License-Apache--2.0%20%7C%20MIT-brightgreen?style=flat-square" alt="Apache-2.0 / MIT"> | 内嵌的 Python 包管理器，构建可重定位 `.venv` |
| **CPython 3.14.6** | <img src="https://img.shields.io/badge/License-PSF--2.0-blue?style=flat-square" alt="PSF-2.0"> | 自动化引擎的 Python 运行时 |
| **Git** | <img src="https://img.shields.io/badge/License-GPL--2.0-blue?style=flat-square" alt="GPL-2.0"> | 仓库代码更新（Unix 源码编译 / Windows MinGit） |
| **Android platform-tools** | <img src="https://img.shields.io/badge/License-Apache--2.0-brightgreen?style=flat-square" alt="Apache-2.0"> | adb 调试桥，模拟器与设备通信 |

### 生态上游

| 依赖组件 | 许可证标识 | 职责定位 |
| :--- | :--- | :--- |
| **AzurPilot** | <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"> | 核心自动化引擎本体（本启动器的服务对象） |
| **AzurLaneAutoScript** | <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"> | 碧蓝航线自动化的上游奠基项目 |

---

## 社区生态与支持

<table width="100%">
  <tr>
    <td width="50%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher/graphs/contributors">
          <img src="https://img.shields.io/badge/Community-Contributors-blueviolet?style=for-the-badge&logo=github&logoColor=white" alt="Contributors">
        </a>
        <br><br>
        <h4>开源共建与参与</h4>
        <p>感谢所有参与代码提交、架构改进、问题排查与功能验证的开发者。欢迎随时提交 Issue 与 PR 参与共建！</p>
        <a href="https://github.com/wess09/AzurPilotLauncher/graphs/contributors">
          <img src="https://img.shields.io/badge/查看完整贡献名单-GitHub%20Graph-181717?style=flat-square&logo=github&logoColor=white" alt="View Contributors">
        </a>
        <a href="https://github.com/wess09/AzurPilotLauncher/issues/new/choose">
          <img src="https://img.shields.io/badge/提交反馈-New%20Issue-0052cc?style=flat-square&logo=githubissues&logoColor=white" alt="New Issue">
        </a>
      </div>
    </td>
    <td width="50%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher/stargazers">
          <img src="https://img.shields.io/badge/Project-Star%20History-f5a623?style=for-the-badge&logo=star&logoColor=white" alt="Star History">
        </a>
        <br><br>
        <h4>项目成长与支持</h4>
        <p>如果本启动器为你的挂机体验带来了便利，欢迎前往仓库主页点亮 Star 支持项目演进。</p>
        <a href="https://star-history.com/#wess09/AzurPilotLauncher&Date">
          <img src="https://img.shields.io/badge/Star%20趋势看板-Star--History-orange?style=flat-square" alt="Star History Link">
        </a>
      </div>
    </td>
  </tr>
</table>

---

## 许可证说明

本项目采用 [GNU General Public License v3.0 (GPL-3.0)](LICENSE) 协议开源——因为 AzurPilot 采用 GPLv3，本启动器与其保持一致。

---

<div align="center">
  <sub>AzurPilot Launcher 遵守开源协议并由社区驱动维护。如遇问题欢迎提交 <a href="https://github.com/wess09/AzurPilotLauncher/issues">Issue</a> 或 <a href="https://github.com/wess09/AzurPilotLauncher/pulls">Pull Request</a>。</sub>
</div>

## 启动与开发

打开启动器后先进入首页；点击“启动”才准备环境和启动 WebUI。首页提供“停止服务”和“修复环境”；修复会重建 .venv，保留配置和日志。启动器更新需点击确认。

开发需要 Node.js 24。

```bash
npm --prefix frontend ci
cargo tauri dev
npm --prefix frontend test
npm --prefix frontend run build
cargo build --locked --release --features custom-protocol
```

[React launcher architecture and IPC](docs/launcher-ui.md)
