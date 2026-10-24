<div align="center">

<img src="icons/icon.png" alt="AzurPilot Launcher Logo" width="200">

# AzurPilot Launcher

**Native Cross-Platform Runner · Zero-Config Embedded Python · One-Click AzurPilot Launch & Update**

A cross-platform desktop launcher for [AzurPilot](https://github.com/wess09/AzurPilot), the full-featured Azur Lane automation assistant based on [AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript). Built with Tauri 2 + Rust, running natively on Windows / macOS / Linux.

<p align="center">
  <a href="README.md">简体中文</a> |
  <b>English</b> |
  <a href="README_ja.md">日本語</a> |
  <a href="README_zh-TW.md">繁體中文</a>
</p>

---

<!-- Core environment and platform badges -->
<p align="center">
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue.svg?style=flat-square" alt="License: GPL-3.0"></a>
  <a href="https://www.tauri.app/"><img src="https://img.shields.io/badge/Framework-Tauri%202-24C8DB.svg?style=flat-square&logo=tauri&logoColor=white" alt="Framework: Tauri 2"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Language-Rust-DEA584.svg?style=flat-square&logo=rust&logoColor=white" alt="Language: Rust"></a>
  <a href="https://www.python.org/"><img src="https://img.shields.io/badge/Python-3.14.6-3776AB.svg?style=flat-square&logo=python&logoColor=white" alt="Python 3.14.6"></a>
  <a href="https://github.com/astral-sh/uv"><img src="https://img.shields.io/badge/Packaging-uv-DE5FE9.svg?style=flat-square" alt="Packaging: uv"></a>
  <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-181717.svg?style=flat-square&logo=github&logoColor=white" alt="Platform: Windows / macOS / Linux">
</p>

<!-- Tech stack and framework badges -->
<p align="center">
  <a href="https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API"><img src="https://img.shields.io/badge/Shell-WebView%20%2B%20Rust%20Backend-24C8DB.svg?style=flat-square" alt="Shell: WebView + Rust"></a>
  <img src="https://img.shields.io/badge/i18n-4%20Languages-blueviolet.svg?style=flat-square" alt="i18n: 4 Languages">
  <img src="https://img.shields.io/badge/Update-Git%20%2B%20uv%20Sync-brightgreen.svg?style=flat-square" alt="Update: Git + uv Sync">
  <img src="https://img.shields.io/badge/Self%20Update-mTLS%20Secured-orange.svg?style=flat-square" alt="Self Update: mTLS">
  <a href="https://deepwiki.com/wess09/AzurPilotLauncher"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki"></a>
</p>

<!-- Repository dynamics and community metric badges -->
<p align="center">
  <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/github/v/release/wess09/AzurPilotLauncher?style=flat-square&color=007ec6&label=Latest%20Release" alt="Latest Release"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/releases"><img src="https://img.shields.io/github/downloads/wess09/AzurPilotLauncher/total?style=flat-square&color=28a745&label=Downloads" alt="Total Downloads"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/stargazers"><img src="https://img.shields.io/github/stars/wess09/AzurPilotLauncher?style=flat-square&color=f5a623&label=Stars" alt="Stars"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/network/members"><img src="https://img.shields.io/github/forks/wess09/AzurPilotLauncher?style=flat-square&color=6f42c1&label=Forks" alt="Forks"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/issues"><img src="https://img.shields.io/github/issues/wess09/AzurPilotLauncher?style=flat-square&color=d73a49&label=Issues" alt="Open Issues"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/commits/main"><img src="https://img.shields.io/github/last-commit/wess09/AzurPilotLauncher?style=flat-square&color=586069&label=Last%20Commit" alt="Last Commit"></a>
</p>

<p align="center">
  <a href="#project-overview-and-metrics">Overview</a> •
  <a href="#project-description">About</a> •
  <a href="#related-ecosystem-projects">Ecosystem</a> •
  <a href="#key-features">Features</a> •
  <a href="#system-architecture">Architecture</a> •
  <a href="#system-requirements">Requirements</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#app-preview">Preview</a> •
  <a href="#differences-from-the-original-launcher">Changes</a> •
  <a href="#directory-layout">Layout</a> •
  <a href="#development-activity">Activity</a> •
  <a href="#star-growth-trend">Star Trend</a> •
  <a href="#community-and-support">Community</a>
</p>

</div>

---

## Project Overview and Metrics

<table width="100%">
  <thead>
    <tr>
      <th colspan="4" align="left">
        <img src="https://img.shields.io/badge/Project%20Overview-AzurPilot%20Launcher-181717?style=flat-square&logo=github&logoColor=white" alt="Overview">
        <b>Core Runtime &amp; Development Metrics</b>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="25%"><b>Latest Release</b><br><a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/github/v/release/wess09/AzurPilotLauncher?style=flat-square&color=007ec6" alt="Release"></a></td>
      <td width="25%"><b>Total Downloads</b><br><a href="https://github.com/wess09/AzurPilotLauncher/releases"><img src="https://img.shields.io/github/downloads/wess09/AzurPilotLauncher/total?style=flat-square&color=28a745" alt="Downloads"></a></td>
      <td width="25%"><b>Open Source License</b><br><a href="./LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"></a></td>
      <td width="25%"><b>Automated Build</b><br><a href="https://github.com/wess09/AzurPilotLauncher/actions"><img src="https://img.shields.io/badge/CI-GitHub%20Actions-2088FF?style=flat-square&logo=githubactions&logoColor=white" alt="CI Status"></a></td>
    </tr>
    <tr>
      <td width="25%"><b>Repository Size</b><br><img src="https://img.shields.io/github/repo-size/wess09/AzurPilotLauncher?style=flat-square&color=586069" alt="Repo Size"></td>
      <td width="25%"><b>Code Language</b><br><img src="https://img.shields.io/badge/Language-Rust%20%7C%20Tauri-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Language"></td>
      <td width="25%"><b>Runtime</b><br><img src="https://img.shields.io/badge/Python-3.14.6%20%2B%20uv-3776AB?style=flat-square&logo=python&logoColor=white" alt="Python Runtime"></td>
      <td width="25%"><b>UI Languages</b><br><img src="https://img.shields.io/badge/i18n-%E7%AE%80%E4%BD%93%20%7C%20%E7%B9%81%E9%AB%94%20%7C%20%E6%97%A5%E6%9C%AC%E8%AA%9E%20%7C%20En-blueviolet?style=flat-square" alt="i18n"></td>
    </tr>
    <tr>
      <td colspan="4">
        <b>Quick Actions:</b>
        <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/badge/Release-Download%20Latest-0052cc?style=flat-square&logo=github&logoColor=white" alt="Download"></a>
        <a href="https://deepwiki.com/wess09/AzurPilotLauncher"><img src="https://img.shields.io/badge/Wiki-Ask%20DeepWiki-6366F1?style=flat-square" alt="Ask DeepWiki"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/issues/new/choose"><img src="https://img.shields.io/badge/Issue-Report%20a%20Bug-d73a49?style=flat-square&logo=githubissues&logoColor=white" alt="New Issue"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/pulls"><img src="https://img.shields.io/badge/PR-Contribute%20Code-28a745?style=flat-square&logo=git&logoColor=white" alt="Pull Request"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/stargazers"><img src="https://img.shields.io/badge/Star-Support%20the%20Project-f5a623?style=flat-square&logo=github&logoColor=white" alt="Star"></a>
      </td>
    </tr>
  </tbody>
</table>

---

## Project Description

**AzurPilot Launcher** makes the full-featured Azur Lane automation assistant **AzurPilot** run natively with one click on Windows, macOS and Linux — whether on an Apple Silicon Mac Mini or a classic x86 machine, with no translation layers, no Docker, and no pollution of your system environment.

The launcher embeds the `uv` package manager and, on first run, automatically creates a relocatable `.venv` (Python 3.14.6), pulls the latest code via git, syncs dependencies, then starts the Python WebUI backend inside a native WebView shell — complete with a splash screen, system tray, desktop notifications and a custom title bar. The UI supports four languages (Simplified Chinese, Traditional Chinese, Japanese and English), selected automatically from the system locale.

---

## Related Ecosystem Projects

This project works closely with the following core projects in its ecosystem:

<table width="100%">
  <tr>
    <td width="33%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher">
          <img src="https://img.shields.io/badge/Repository-AzurPilot%20Launcher-181717?style=for-the-badge&logo=github&logoColor=white" alt="AzurPilot Launcher">
        </a>
      </div>
      <br>
      <b>Cross-Platform Launcher (this project)</b>
      <p>An all-in-one desktop launcher for Windows / macOS / Linux, covering environment setup, code updates, dependency sync and WebUI lifecycle management.</p>
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
      <b>Core Automation Engine</b>
      <p>The Azur Lane automation assistant core, featuring complete computer-vision recognition, sortie scheduling algorithms and a local web console service.</p>
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
      <b>Upstream Automation Script (AzurLaneAutoScript)</b>
      <p>The upstream AzurLaneAutoScript project — the foundation of Azur Lane automation, from which this ecosystem's capabilities evolved.</p>
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

## Key Features

<table width="100%">
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/Module-Deployment-0052cc?style=flat-square" alt="Tag"><br>
      <h3>Zero-Config Environment Setup</h3>
      <p>The launcher embeds <code>uv</code>. On first run it automatically downloads Python 3.14.6 and creates a <b>relocatable <code>.venv</code></b>, deploying uv, git and adb into the same virtual environment. No pre-installed Python, uv or Git required — it just works out of the box.</p>
      <div>
        <img src="https://img.shields.io/badge/Runtime-Python%203.14.6-orange?style=flat-square" alt="Python">
        <img src="https://img.shields.io/badge/Dependencies-Embedded%20uv-blueviolet?style=flat-square" alt="uv">
        <img src="https://img.shields.io/badge/Experience-Ready%20to%20Run-brightgreen?style=flat-square" alt="Ready">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/Module-Native%20Shell-00875a?style=flat-square" alt="Tag"><br>
      <h3>Native Desktop Shell Experience</h3>
      <p>A native WebView shell built on Tauri 2: glassmorphism splash screen and custom title bar, system tray, Windows Toast / macOS / Linux native notifications, and single-instance behavior — relaunching simply focuses the existing window.</p>
      <div>
        <img src="https://img.shields.io/badge/Framework-Tauri%202-blue?style=flat-square" alt="Tauri">
        <img src="https://img.shields.io/badge/Integration-Tray%20%2B%20Notify%20%2B%20Titlebar-teal?style=flat-square" alt="Native">
        <img src="https://img.shields.io/badge/Instance-Single%20Instance-lightgrey?style=flat-square" alt="Single Instance">
      </div>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/Module-Update%20System-403294?style=flat-square" alt="Tag"><br>
      <h3>Automated Update System</h3>
      <p>On launch, the latest code is pulled via git (up to 20 retries), then dependencies are synced exactly with <code>uv sync --frozen</code> against <code>uv.lock</code>. The launcher itself self-updates over an mTLS-secured channel with multi-mirror fallback.</p>
      <div>
        <img src="https://img.shields.io/badge/Code%20Update-Git%20Retry%20Mechanism-purple?style=flat-square" alt="Git Update">
        <img src="https://img.shields.io/badge/Locking-uv.lock-blue?style=flat-square" alt="Lock">
        <img src="https://img.shields.io/badge/Self%20Update-mTLS%20Multi%20Mirror-orange?style=flat-square" alt="mTLS">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/Module-Internationalization-172b4d?style=flat-square" alt="Tag"><br>
      <h3>Four-Language Interface</h3>
      <p>Built-in UI localization for 简体中文, 繁體中文, 日本語 and English, chosen automatically from the system locale or forced via the <code>--lang</code> command-line flag. WebUI and web notifications follow the selected language.</p>
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
      <img src="https://img.shields.io/badge/Module-Network%20Strategy-de350b?style=flat-square" alt="Tag"><br>
      <h3>Dual Mirrors &amp; Direct-Connect Policy</h3>
      <p>Two deployment profiles are provided: the international build and the <code>-cn</code> China-mirror build. Python standalone downloads are accelerated via npmmirror by default. Code updates, embedded Python, uv and dependency installs, plus WebView-to-WebUI connections all bypass the system proxy for a cleaner environment.</p>
      <div>
        <img src="https://img.shields.io/badge/Profiles-International%20%2F%20CN%20Mirror-red?style=flat-square" alt="Mirrors">
        <img src="https://img.shields.io/badge/Acceleration-npmmirror-red?style=flat-square" alt="npmmirror">
        <img src="https://img.shields.io/badge/Connection-Smart%20Direct-green?style=flat-square" alt="Direct">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/Module-Lifecycle-ff5630?style=flat-square" alt="Tag"><br>
      <h3>Robust Process &amp; Window Management</h3>
      <p>Port-blocking processes are cleaned up before the backend starts. Child processes are tagged via an environment variable and swept from the full process table on exit, so <code>gui.py</code> never leaks. The WebUI opens pre-authenticated through a one-time token — the window is logged in the moment it appears.</p>
      <div>
        <img src="https://img.shields.io/badge/Ports-Auto%20Cleanup-blue?style=flat-square" alt="Port Clean">
        <img src="https://img.shields.io/badge/Processes-Leak%20Free-brightgreen?style=flat-square" alt="Leak Free">
        <img src="https://img.shields.io/badge/WebUI-Auth%20Free-009688?style=flat-square" alt="Auth Free">
      </div>
    </td>
  </tr>
</table>

---

## System Architecture

The project follows a cleanly decoupled layered design with well-defined responsibilities and communication boundaries:

```mermaid
graph TD
    subgraph Shell ["Launcher Shell (Tauri 2 · Rust)"]
        Splash["Splash Screen React launcher<br/>(Glass Progress Bar / Error Page React connection error)"]
        Setup["Environment Setup setup.rs<br/>(Python / uv / adb / git Install & Migration)"]
        Tray["System Integration<br/>(Tray / Native Notifications / Custom Titlebar)"]
        Backend["Backend Manager backend.rs<br/>(Port Cleanup / Process Reclaim)"]
    end

    subgraph Venv [".venv Runtime (uv-managed · Relocatable)"]
        PYTHON["CPython 3.14.6 Interpreter"]
        TOOLS["Embedded uv / git / adb Toolchain"]
        SYNC["uv sync --frozen Dependency Sync (uv.lock)"]
    end

    subgraph Repo ["AzurLaneAutoScript Repository"]
        GIT["Git Auto-Update (deploy.git.GitManager · 20 Retries)"]
        GUI["gui.py WebUI Backend"]
        WEB["WebUI Console (127.0.0.1:22267)"]
    end

    Splash --> Setup
    Setup --> TOOLS
    TOOLS --> PYTHON
    Setup --> GIT
    GIT --> SYNC
    SYNC --> GUI
    Backend --> GUI
    GUI --> WEB
    Shell -. "WebView Hosting · SSE Notification Stream" .-> WEB
    Tray -. "Desktop Notifications / Tray Control" .-> Shell
```

---

## System Requirements

<table width="100%">
  <thead>
    <tr>
      <th width="20%">Dimension</th>
      <th width="40%">Requirement</th>
      <th width="40%">Notes</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td><b>Operating System</b></td>
      <td><img src="https://img.shields.io/badge/Windows-10%2B%20(x64%20%2F%20ARM64)-0078D6?style=flat-square&logo=windows&logoColor=white" alt="Windows"> <img src="https://img.shields.io/badge/macOS-Native-000000?style=flat-square&logo=apple&logoColor=white" alt="macOS"> <img src="https://img.shields.io/badge/Linux-x86__64%20%2F%20ARM64-FCC624?style=flat-square&logo=linux&logoColor=black" alt="Linux"></td>
      <td>CI builds on an Ubuntu 22.04 / macOS / latest Windows matrix</td>
    </tr>
    <tr>
      <td><b>WebView Runtime</b></td>
      <td><img src="https://img.shields.io/badge/Windows-WebView2-0078D6?style=flat-square" alt="WebView2"> <img src="https://img.shields.io/badge/Linux-libwebkit2gtk--4.1-FCC624?style=flat-square" alt="libwebkit2gtk"> <img src="https://img.shields.io/badge/macOS-Built%20In-000000?style=flat-square" alt="WKWebView"></td>
      <td>Windows 7/8/10 requires <a href="https://developer.microsoft.com/en-us/microsoft-edge/webview2">WebView2</a> installed first; Linux needs a recent glibc</td>
    </tr>
    <tr>
      <td><b>Python / uv / Git / adb</b></td>
      <td><img src="https://img.shields.io/badge/Preinstall-Not%20Required-brightgreen?style=flat-square" alt="No Preinstall"></td>
      <td>The launcher embeds uv, auto-creates the <code>.venv</code>, and deploys git &amp; adb into it</td>
    </tr>
    <tr>
      <td><b>Network</b></td>
      <td><img src="https://img.shields.io/badge/First%20Launch-Online%20Required-yellow?style=flat-square" alt="Network"></td>
      <td>Needed to download Python, sync dependencies and update the repo; the CN build uses China mirrors</td>
    </tr>
    <tr>
      <td><b>Node.js (Windows only)</b></td>
      <td><img src="https://img.shields.io/badge/Detection-Automatic-orange?style=flat-square&logo=nodedotjs&logoColor=white" alt="Node.js"></td>
      <td>If no system-level Node.js is found, the launcher offers to download and install it (SHA-256 verified)</td>
    </tr>
  </tbody>
</table>

---

## Quick Start

<table width="100%">
  <tr>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/Step-01-blue?style=flat-square" alt="Step 1"><br>
        <h4>Get the Archive</h4>
      </div>
      Download the archive <b>matching your OS and CPU</b> (international or CN-mirror build) from the <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest">latest release</a>.
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/Step-02-indigo?style=flat-square" alt="Step 2"><br>
        <h4>Extract &amp; Launch</h4>
      </div>
      Run <code>AzurPilot.exe</code> on Windows (administrator required); open <code>AzurPilot.app</code> on macOS; run <code>AzurPilot</code> on Linux.
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/Step-03-purple?style=flat-square" alt="Step 3"><br>
        <h4>Wait for Setup</h4>
      </div>
      The splash screen shows live progress: creating the <code>.venv</code>, updating the repo, syncing dependencies. First launch requires a network connection; subsequent launches simply focus the existing window.
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/Step-04-green?style=flat-square" alt="Step 4"><br>
        <h4>Enter the WebUI</h4>
      </div>
      The window opens already logged in to the AzurPilot WebUI — configure sortie tasks and schedules and start automated cruising.
    </td>
  </tr>
</table>

> [!TIP]
> The launcher supports command-line flags: `--lang` to force a UI language (`zh-CN` / `zh-TW` / `ja` / `en`), `--skip-update` to skip the repository update, and `--preview-crash` to preview the error page.

> [!WARNING]
> **macOS users**: the app is not signed by a developer certificate. If it fails to open, run `xattr -dr com.apple.quarantine AzurPilot.app` in a terminal first.

> [!IMPORTANT]
> **Linux users**: the launcher depends on `libwebkit2gtk-4.1` and a reasonably recent `glibc` (CI builds on Ubuntu 22.04). If missing system libraries prevent the launcher from running, the AzurPilot core can still be used normally from the command line.

---

## App Preview

| Platform | 简体中文 | English |
|:---:|:---:|:---:|
| **Windows** | <img src="screenshots/win-cn.webp" width="420"/> | <img src="screenshots/win-en.webp" width="420"/> |
| **macOS** | <img src="screenshots/mac-cn.webp" width="420"/> | <img src="screenshots/mac-en.webp" width="420"/> |

---

## Differences from the Original Launcher

Compared with the bundled Electron launcher of [AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript):

| # | Change | Description |
|:---:|:---|:---|
| 1 | **Cross-platform** | No longer Windows-only — runs natively on macOS (including Apple Silicon) and Linux |
| 2 | **Reworked update strategy** | Only the git repo is updated, and dependencies are synced with the uv embedded in `.venv`; relaunching merely refocuses the existing window instead of doing destructive cleanup |
| 3 | **Pinned dependency versions** | Python package versions are locked by `pyproject.toml` and `uv.lock`, with auto-sync enabled by default to prevent environment drift |
| 4 | **adb policy** | adb is no longer restarted or replaced |
| 5 | **Adjusted directory layout** | Python / uv / git / adb all live inside `.venv` — see the layout below |
| 6 | **Node.js automation (Windows)** | Detects system-level Node.js; if absent, offers to download and install it automatically after confirmation |

---

## Directory Layout

```
AzurPilot root directory
* Windows: AzurLaneAutoScript
* macOS:   AzurPilot.app/Contents/AzurLaneAutoScript
* Linux:   AzurLaneAutoScript

AzurPilot launcher
* Windows: AzurLaneAutoScript/AzurPilot.exe
* macOS:   AzurPilot.app/Contents/MacOS/AzurPilot
* Linux:   AzurLaneAutoScript/AzurPilot

Python / uv
* All systems: .venv

Git
* Unix:    .venv/bin/git
* Windows: .venv/Scripts/git/cmd/git.exe

Adb
* Unix:    .venv/bin/adb
* Windows: .venv/Scripts/adb.exe

Environment variables appended by the launcher
* Unix:    .venv/bin
* Windows: .venv/Scripts, .venv/Scripts/git/cmd
```

---

## Development Activity

<table width="100%">
  <thead>
    <tr>
      <th colspan="3" align="left">
        <img src="https://img.shields.io/badge/Metrics-Repo%20Activity-5C3EE8?style=flat-square&logo=github&logoColor=white" alt="Metrics">
        <b>Repository Activity &amp; Responsiveness</b>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="33%">
        <b>Commit Frequency</b><br>
        <img src="https://img.shields.io/github/commit-activity/m/wess09/AzurPilotLauncher?style=flat-square&color=00d4aa" alt="Commit Activity"><br>
        <small>Monthly commit activity</small>
      </td>
      <td width="33%">
        <b>Pull Requests</b><br>
        <img src="https://img.shields.io/github/issues-pr-closed/wess09/AzurPilotLauncher?style=flat-square&color=6f42c1" alt="Closed PRs"><br>
        <small>Total closed pull requests</small>
      </td>
      <td width="33%">
        <b>Issues Resolved</b><br>
        <img src="https://img.shields.io/github/issues-closed/wess09/AzurPilotLauncher?style=flat-square&color=28a745" alt="Closed Issues"><br>
        <small>Total resolved issues</small>
      </td>
    </tr>
    <tr>
      <td width="33%">
        <b>Development Branch</b><br>
        <img src="https://img.shields.io/badge/Branch-main-181717?style=flat-square&logo=git&logoColor=white" alt="Main Branch"><br>
        <small>Continuous delivery trunk</small>
      </td>
      <td width="33%">
        <b>Automated Builds</b><br>
        <img src="https://img.shields.io/badge/CI%2FCD-GitHub%20Actions-2088FF?style=flat-square&logo=githubactions&logoColor=white" alt="CI"><br>
        <small>Three-platform build matrix</small>
      </td>
      <td width="33%">
        <b>Latest Commit</b><br>
        <a href="https://github.com/wess09/AzurPilotLauncher/commits/main"><img src="https://img.shields.io/github/last-commit/wess09/AzurPilotLauncher?style=flat-square&color=586069" alt="Last Commit"></a><br>
        <small>Tracking the latest evolution</small>
      </td>
    </tr>
  </tbody>
</table>

---

## Star Growth Trend

An adaptive light/dark Star-History chart reflecting the project's growth:

<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date" />
    <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date" width="100%" />
  </picture>
  <br>
  <sub>Live data from <a href="https://star-history.com/#wess09/AzurPilotLauncher&Date">Star-History</a> · click for the interactive full chart</sub>
</div>

---

## Tech Stack and Acknowledgements

### Launcher Tech Stack

| Component | License | Role |
| :--- | :--- | :--- |
| **Tauri 2** | <img src="https://img.shields.io/badge/License-MIT%20%7C%20Apache--2.0-brightgreen?style=flat-square" alt="MIT / Apache-2.0"> | Cross-platform desktop shell, WebView hosting and system integration |
| **Rust** | <img src="https://img.shields.io/badge/License-MIT%20%7C%20Apache--2.0-brightgreen?style=flat-square" alt="MIT / Apache-2.0"> | Systems programming language powering all native launcher logic |
| **uv** | <img src="https://img.shields.io/badge/License-Apache--2.0%20%7C%20MIT-brightgreen?style=flat-square" alt="Apache-2.0 / MIT"> | Embedded Python package manager building the relocatable `.venv` |
| **CPython 3.14.6** | <img src="https://img.shields.io/badge/License-PSF--2.0-blue?style=flat-square" alt="PSF-2.0"> | The Python runtime for the automation engine |
| **Git** | <img src="https://img.shields.io/badge/License-GPL--2.0-blue?style=flat-square" alt="GPL-2.0"> | Repository updates (compiled from source on Unix / MinGit on Windows) |
| **Android platform-tools** | <img src="https://img.shields.io/badge/License-Apache--2.0-brightgreen?style=flat-square" alt="Apache-2.0"> | The adb bridge for emulator and device communication |

### Upstream Ecosystem

| Component | License | Role |
| :--- | :--- | :--- |
| **AzurPilot** | <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"> | The core automation engine (what this launcher serves) |
| **AzurLaneAutoScript** | <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"> | The upstream foundation of Azur Lane automation |

---

## Community and Support

<table width="100%">
  <tr>
    <td width="50%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher/graphs/contributors">
          <img src="https://img.shields.io/badge/Community-Contributors-blueviolet?style=for-the-badge&logo=github&logoColor=white" alt="Contributors">
        </a>
        <br><br>
        <h4>Open Source Collaboration</h4>
        <p>Thanks to every developer who has contributed code, improved the architecture, debugged issues or verified features. Feel free to open an Issue or PR to join in!</p>
        <a href="https://github.com/wess09/AzurPilotLauncher/graphs/contributors">
          <img src="https://img.shields.io/badge/View%20Contributors-GitHub%20Graph-181717?style=flat-square&logo=github&logoColor=white" alt="View Contributors">
        </a>
        <a href="https://github.com/wess09/AzurPilotLauncher/issues/new/choose">
          <img src="https://img.shields.io/badge/Give%20Feedback-New%20Issue-0052cc?style=flat-square&logo=githubissues&logoColor=white" alt="New Issue">
        </a>
      </div>
    </td>
    <td width="50%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher/stargazers">
          <img src="https://img.shields.io/badge/Project-Star%20History-f5a623?style=for-the-badge&logo=star&logoColor=white" alt="Star History">
        </a>
        <br><br>
        <h4>Growth &amp; Support</h4>
        <p>If this launcher makes your automated cruising smoother, please consider starring the repository to support its development.</p>
        <a href="https://star-history.com/#wess09/AzurPilotLauncher&Date">
          <img src="https://img.shields.io/badge/Star%20Trends-Star--History-orange?style=flat-square" alt="Star History Link">
        </a>
      </div>
    </td>
  </tr>
</table>

---

## License

This project is open-sourced under the [GNU General Public License v3.0 (GPL-3.0)](LICENSE) — consistent with AzurPilot, which is also GPLv3.

---

<div align="center">
  <sub>AzurPilot Launcher is open-source and community-driven. For any issues, feel free to open an <a href="https://github.com/wess09/AzurPilotLauncher/issues">Issue</a> or a <a href="https://github.com/wess09/AzurPilotLauncher/pulls">Pull Request</a>.</sub>
</div>

## Launching and development

The launcher opens a home page. Click Launch to prepare the environment and open WebUI. Stop the service before repairing .venv; settings and logs are preserved. Launcher updates require an explicit click.

Development requires Node.js 24.

```bash
npm --prefix frontend ci
cargo tauri dev
npm --prefix frontend test
npm --prefix frontend run build
cargo build --locked --release --features custom-protocol
```

[React launcher architecture and IPC](docs/launcher-ui.md)
