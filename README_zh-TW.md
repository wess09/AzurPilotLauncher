<div align="center">

<img src="icons/icon.png" alt="AzurPilot Launcher Logo" width="200">

# AzurPilot Launcher

**全平台原生執行 · 內建 Python 環境零設定 · 一鍵更新啟動 AzurPilot**

全功能《碧藍航線》自動化助手 [AzurPilot](https://github.com/wess09/AzurPilot)（基於 [AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript)）的跨平台桌面啟動器，基於 Tauri 2 + Rust 建構，支援 Windows / macOS / Linux。

<p align="center">
  <a href="README.md">简体中文</a> |
  <a href="README_en.md">English</a> |
  <a href="README_ja.md">日本語</a> |
  <b>繁體中文</b>
</p>

---

<!-- 核心環境與平台標籤組 -->
<p align="center">
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue.svg?style=flat-square" alt="License: GPL-3.0"></a>
  <a href="https://www.tauri.app/"><img src="https://img.shields.io/badge/Framework-Tauri%202-24C8DB.svg?style=flat-square&logo=tauri&logoColor=white" alt="Framework: Tauri 2"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Language-Rust-DEA584.svg?style=flat-square&logo=rust&logoColor=white" alt="Language: Rust"></a>
  <a href="https://www.python.org/"><img src="https://img.shields.io/badge/Python-3.14.x-3776AB.svg?style=flat-square&logo=python&logoColor=white" alt="Python 3.14.x"></a>
  <a href="https://github.com/astral-sh/uv"><img src="https://img.shields.io/badge/Packaging-uv-DE5FE9.svg?style=flat-square" alt="Packaging: uv"></a>
  <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-181717.svg?style=flat-square&logo=github&logoColor=white" alt="Platform: Windows / macOS / Linux">
</p>

<!-- 技術棧與框架標籤組 -->
<p align="center">
  <a href="https://developer.mozilla.org/zh-TW/docs/Web/API/Web_Workers_API"><img src="https://img.shields.io/badge/Shell-WebView%20%2B%20Rust%20Backend-24C8DB.svg?style=flat-square" alt="Shell: WebView + Rust"></a>
  <img src="https://img.shields.io/badge/i18n-4%20Languages-blueviolet.svg?style=flat-square" alt="i18n: 4 Languages">
  <img src="https://img.shields.io/badge/Update-Git%20%2B%20uv%20Sync-brightgreen.svg?style=flat-square" alt="Update: Git + uv Sync">
  <img src="https://img.shields.io/badge/Self%20Update-mTLS%20Secured-orange.svg?style=flat-square" alt="Self Update: mTLS">
  <a href="https://deepwiki.com/wess09/AzurPilotLauncher"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki"></a>
</p>

<!-- 儲存庫動態與社群指標標籤組 -->
<p align="center">
  <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/github/v/release/wess09/AzurPilotLauncher?style=flat-square&color=007ec6&label=Latest%20Release" alt="Latest Release"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/releases"><img src="https://img.shields.io/github/downloads/wess09/AzurPilotLauncher/total?style=flat-square&color=28a745&label=Downloads" alt="Total Downloads"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/stargazers"><img src="https://img.shields.io/github/stars/wess09/AzurPilotLauncher?style=flat-square&color=f5a623&label=Stars" alt="Stars"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/network/members"><img src="https://img.shields.io/github/forks/wess09/AzurPilotLauncher?style=flat-square&color=6f42c1&label=Forks" alt="Forks"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/issues"><img src="https://img.shields.io/github/issues/wess09/AzurPilotLauncher?style=flat-square&color=d73a49&label=Issues" alt="Open Issues"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/commits/main"><img src="https://img.shields.io/github/last-commit/wess09/AzurPilotLauncher?style=flat-square&color=586069&label=Last%20Commit" alt="Last Commit"></a>
</p>

<p align="center">
  <a href="#專案總覽與指標">專案總覽</a> •
  <a href="#專案概述">專案概述</a> •
  <a href="#關聯生態工程">關聯生態</a> •
  <a href="#核心特性">核心特性</a> •
  <a href="#系統架構全景">系統架構</a> •
  <a href="#環境需求">環境需求</a> •
  <a href="#快速開始">快速開始</a> •
  <a href="#應用預覽">應用預覽</a> •
  <a href="#與原版啟動器的差異">版本差異</a> •
  <a href="#目錄結構">目錄結構</a> •
  <a href="#開發活躍度">開發活躍</a> •
  <a href="#star-成長趨勢">成長趨勢</a> •
  <a href="#社群生態與支援">社群支援</a>
</p>

</div>

---

## 專案總覽與指標

<table width="100%">
  <thead>
    <tr>
      <th colspan="4" align="left">
        <img src="https://img.shields.io/badge/Project%20Overview-AzurPilot%20Launcher-181717?style=flat-square&logo=github&logoColor=white" alt="Overview">
        <b>工程核心執行與研發指標</b>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="25%"><b>最新正式版</b><br><a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/github/v/release/wess09/AzurPilotLauncher?style=flat-square&color=007ec6" alt="Release"></a></td>
      <td width="25%"><b>累計下載量</b><br><a href="https://github.com/wess09/AzurPilotLauncher/releases"><img src="https://img.shields.io/github/downloads/wess09/AzurPilotLauncher/total?style=flat-square&color=28a745" alt="Downloads"></a></td>
      <td width="25%"><b>開源授權</b><br><a href="./LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"></a></td>
      <td width="25%"><b>自動化建構</b><br><a href="https://github.com/wess09/AzurPilotLauncher/actions"><img src="https://img.shields.io/badge/CI-GitHub%20Actions-2088FF?style=flat-square&logo=githubactions&logoColor=white" alt="CI Status"></a></td>
    </tr>
    <tr>
      <td width="25%"><b>儲存庫體積</b><br><img src="https://img.shields.io/github/repo-size/wess09/AzurPilotLauncher?style=flat-square&color=586069" alt="Repo Size"></td>
      <td width="25%"><b>程式語言</b><br><img src="https://img.shields.io/badge/Language-Rust%20%7C%20Tauri-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Language"></td>
      <td width="25%"><b>執行環境</b><br><img src="https://img.shields.io/badge/Python-3.14.x%20%2B%20uv-3776AB?style=flat-square&logo=python&logoColor=white" alt="Python Runtime"></td>
      <td width="25%"><b>介面語言</b><br><img src="https://img.shields.io/badge/i18n-%E7%B0%A1%E9%AB%94%20%7C%20%E7%B9%81%E9%AB%94%20%7C%20%E6%97%A5%E6%9C%AC%E8%AA%9E%20%7C%20En-blueviolet?style=flat-square" alt="i18n"></td>
    </tr>
    <tr>
      <td colspan="4">
        <b>快速操作快捷入口：</b>
        <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/badge/Release-下載最新壓縮檔-0052cc?style=flat-square&logo=github&logoColor=white" alt="Download"></a>
        <a href="https://deepwiki.com/wess09/AzurPilotLauncher"><img src="https://img.shields.io/badge/Wiki-Ask%20DeepWiki-6366F1?style=flat-square" alt="Ask DeepWiki"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/issues/new/choose"><img src="https://img.shields.io/badge/Issue-提交錯誤回報與需求-d73a49?style=flat-square&logo=githubissues&logoColor=white" alt="New Issue"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/pulls"><img src="https://img.shields.io/badge/PR-合併請求程式碼貢獻-28a745?style=flat-square&logo=git&logoColor=white" alt="Pull Request"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/stargazers"><img src="https://img.shields.io/badge/Star-關注專案發展-f5a623?style=flat-square&logo=github&logoColor=white" alt="Star"></a>
      </td>
    </tr>
  </tbody>
</table>

---

## 專案概述

**AzurPilot Launcher** 致力於讓全功能《碧藍航線》自動化助手 **AzurPilot** 在 Windows、macOS 與 Linux 上以原生方式一鍵執行——無論是 Apple Silicon 的 Mac Mini，還是傳統 x86 主機，都無需轉譯層、無需 Docker、不污染系統環境。

啟動器透過同一份 `stable.json` 按需下載 Git/ADB 二進位、deploy 腳本和設定範本，再由內建 uv 在本機安裝最新穩定 Python 3.14 修補版與 Python 依賴，透過 deploy 腳本更新倉庫和建置 WebUI。缺少引導檔案時顯示「部署」；系統為簡體中文且使用北京時間時優先 CNB，其餘優先 GitHub。設定可選擇元件、Python 與 PyPI 鏡像，僅顯示鏡像名稱。無需預裝 Git、uv 或 Python。詳見[部署引導元件](docs/runtime-components.md)。

---

## 關聯生態工程

本工程與相關聯的核心專案緊密聯動，關鍵依賴與來源專案如下：

<table width="100%">
  <tr>
    <td width="33%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher">
          <img src="https://img.shields.io/badge/Repository-AzurPilot%20Launcher-181717?style=for-the-badge&logo=github&logoColor=white" alt="AzurPilot Launcher">
        </a>
      </div>
      <br>
      <b>跨平台啟動器 (當前專案)</b>
      <p>面向 Windows / macOS / Linux 的一體化桌面啟動器，打通環境建置、程式碼更新、依賴同步與 WebUI 生命週期管理。</p>
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
      <b>核心自動化引擎本體</b>
      <p>《碧藍航線》自動化助手核心，包含完備的電腦視覺圖像識別、出擊調度演算法與本機 Web 控制台服務。</p>
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
      <b>上游自動化腳本 (AzurLaneAutoScript)</b>
      <p>AzurLaneAutoScript 上游專案，碧藍航線自動化的奠基之作，本生態鏈的全部自動化能力由此演化而來。</p>
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
      <img src="https://img.shields.io/badge/模組-部署形態-0052cc?style=flat-square" alt="Tag"><br>
      <h3>零設定環境建置</h3>
      <p>CI 僅打包 Git/ADB 部署工具。啟動器補齊引導檔案，由內建 uv 安裝 Python 與依賴，並重用已完成的安裝與快取。</p>
      <div>
        <img src="https://img.shields.io/badge/執行環境-Python%203.14.x-orange?style=flat-square" alt="Python">
        <img src="https://img.shields.io/badge/依賴管理-uv%20內建-blueviolet?style=flat-square" alt="uv">
        <img src="https://img.shields.io/badge/體驗-開箱即跑-brightgreen?style=flat-square" alt="Ready">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模組-原生體驗-00875a?style=flat-square" alt="Tag"><br>
      <h3>原生桌面殼層體驗</h3>
      <p>基於 Tauri 2 的原生 WebView 殼：玻璃質感啟動畫面與自訂標題列、系統列、Windows Toast / macOS / Linux 原生通知、單一實例執行，重複啟動自動聚焦已有視窗。</p>
      <div>
        <img src="https://img.shields.io/badge/框架-Tauri%202-blue?style=flat-square" alt="Tauri">
        <img src="https://img.shields.io/badge/互動-系統列%20%2B%20通知%20%2B%20標題列-teal?style=flat-square" alt="Native">
        <img src="https://img.shields.io/badge/實例-單一實例-lightgrey?style=flat-square" alt="Single Instance">
      </div>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模組-更新體系-403294?style=flat-square" alt="Tag"><br>
      <h3>自動化更新體系</h3>
      <p>同一份 stable.json 儲存啟動器與執行元件更新；啟動器提供 CNB 與 GitHub 下載地址。</p>
      <div>
        <img src="https://img.shields.io/badge/程式碼更新-Git%20重試機制-purple?style=flat-square" alt="Git Update">
        <img src="https://img.shields.io/badge/依賴鎖定-uv.lock-blue?style=flat-square" alt="Lock">
        <img src="https://img.shields.io/badge/自我更新-mTLS%20多鏡像-orange?style=flat-square" alt="mTLS">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模組-國際化-172b4d?style=flat-square" alt="Tag"><br>
      <h3>四語言國際化介面</h3>
      <p>內建簡體中文、繁體中文、日本語、English 四種介面語言，依系統 locale 自動選擇，亦可透過 <code>--lang</code> 啟動參數強制指定。WebUI 與 Web 通知依所選語言在地化呈現。</p>
      <div>
        <img src="https://img.shields.io/badge/語言-簡體中文-red?style=flat-square" alt="zh-CN">
        <img src="https://img.shields.io/badge/語言-繁體中文-blue?style=flat-square" alt="zh-TW">
        <img src="https://img.shields.io/badge/言語-日本語-pink?style=flat-square" alt="ja">
        <img src="https://img.shields.io/badge/Language-English-green?style=flat-square" alt="en">
      </div>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模組-網路策略-de350b?style=flat-square" alt="Tag"><br>
      <h3>雙鏡像與直連策略</h3>
      <p>系統為簡體中文且使用北京時間時優先 CNB，其餘優先 GitHub。設定提供元件、Python 與 PyPI 的鏡像名稱。</p>
      <div>
        <img src="https://img.shields.io/badge/部署-國際版%20%2F%20CN%20鏡像-red?style=flat-square" alt="Mirrors">
        <img src="https://img.shields.io/badge/加速-npmmirror-red?style=flat-square" alt="npmmirror">
        <img src="https://img.shields.io/badge/連線-智慧直連-green?style=flat-square" alt="Direct">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/模組-生命週期-ff5630?style=flat-square" alt="Tag"><br>
      <h3>穩健的程序與視窗管理</h3>
      <p>啟動後端前自動清理連接埠占用程序；經環境變數標記子程序歸屬，結束時全程序表掃描回收，杜絕 <code>gui.py</code> 殘留；啟動 WebUI 免密直達（一次性權杖預置登入狀態），視窗開啟即已登入。</p>
      <div>
        <img src="https://img.shields.io/badge/連接埠-占用自動清理-blue?style=flat-square" alt="Port Clean">
        <img src="https://img.shields.io/badge/程序-零殘留-brightgreen?style=flat-square" alt="Leak Free">
        <img src="https://img.shields.io/badge/WebUI-免密直達-009688?style=flat-square" alt="Auth Free">
      </div>
    </td>
  </tr>
</table>

---

## 系統架構全景

專案採用清晰的分層解耦設計，各層間職責清晰、通訊邊界規範：

```mermaid
graph TD
    subgraph Shell ["啟動器殼層 (Tauri 2 · Rust)"]
        Splash["啟動畫面 React launcher<br/>(玻璃質感進度條 / 錯誤頁 React connection error)"]
        Setup["環境建置 setup.rs<br/>(Python / uv / adb / git 安裝與遷移)"]
        Tray["系統整合<br/>(系統列 / 原生通知 / 自訂標題列)"]
        Backend["後端託管 backend.rs<br/>(連接埠清理 / 程序回收)"]
    end

    subgraph Venv [".venv 執行環境 (uv 管理 · 可重定位)"]
        PYTHON["CPython 3.14.x 直譯器"]
        TOOLS["Python / dependencies / ADB 內建工具鏈"]
        SYNC["uv sync --frozen 依賴同步 (uv.lock 鎖定)"]
    end

    subgraph Repo ["AzurLaneAutoScript 儲存庫"]
        GIT["Git 自動更新 (deploy.git.GitManager · 重試 20 次)"]
        GUI["gui.py WebUI 後端"]
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
    Shell -. "WebView 承載 · SSE 通知流" .-> WEB
    Tray -. "桌面通知 / 系統列控制" .-> Shell
```

---

## 環境需求

<table width="100%">
  <thead>
    <tr>
      <th width="20%">面向</th>
      <th width="40%">准入指標</th>
      <th width="40%">說明</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td><b>作業系統</b></td>
      <td><img src="https://img.shields.io/badge/Windows-10%2B%20(x64%20%2F%20ARM64)-0078D6?style=flat-square&logo=windows&logoColor=white" alt="Windows"> <img src="https://img.shields.io/badge/macOS-原生支援-000000?style=flat-square&logo=apple&logoColor=white" alt="macOS"> <img src="https://img.shields.io/badge/Linux-x86__64%20%2F%20ARM64-FCC624?style=flat-square&logo=linux&logoColor=black" alt="Linux"></td>
      <td>CI 以 Ubuntu 22.04 / macOS / Windows 最新版矩陣建構</td>
    </tr>
    <tr>
      <td><b>WebView 執行環境</b></td>
      <td><img src="https://img.shields.io/badge/Windows-WebView2-0078D6?style=flat-square" alt="WebView2"> <img src="https://img.shields.io/badge/Linux-libwebkit2gtk--4.1-FCC624?style=flat-square" alt="libwebkit2gtk"> <img src="https://img.shields.io/badge/macOS-系統內建-000000?style=flat-square" alt="WKWebView"></td>
      <td>Windows 7/8/10 需先安裝 <a href="https://developer.microsoft.com/zh-cn/microsoft-edge/webview2">WebView2</a>；Linux 需較新 glibc</td>
    </tr>
    <tr>
      <td><b>Python / uv / Git / adb</b></td>
      <td><img src="https://img.shields.io/badge/依賴-無需預裝-brightgreen?style=flat-square" alt="No Preinstall"></td>
      <td>啟動器內嵌 uv 並自動建立 <code>.venv</code>，自動部署 git 與 adb 至虛擬環境</td>
    </tr>
    <tr>
      <td><b>網路連線</b></td>
      <td><img src="https://img.shields.io/badge/首次啟動-需連網-yellow?style=flat-square" alt="Network"></td>
      <td>下載 Python、同步依賴與更新儲存庫；CN 版走國內鏡像加速</td>
    </tr>
    <tr>
      <td><b>Node.js (僅 Windows)</b></td>
      <td><img src="https://img.shields.io/badge/偵測-自動處理-orange?style=flat-square&logo=nodedotjs&logoColor=white" alt="Node.js"></td>
      <td>未偵測到系統級 Node.js 時，確認後自動下載安裝（SHA-256 驗證）</td>
    </tr>
  </tbody>
</table>

---

## 快速開始

<table width="100%">
  <tr>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/步驟-01-blue?style=flat-square" alt="Step 1"><br>
        <h4>取得壓縮檔</h4>
      </div>
      前往 <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest">Releases 最新版本</a> 下載<b>對應系統與 CPU 的壓縮檔</b>（國際版或 CN 鏡像版）。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/步驟-02-indigo?style=flat-square" alt="Step 2"><br>
        <h4>解壓縮並啟動</h4>
      </div>
      Windows 執行 <code>AzurPilot.exe</code>（需管理員權限）；macOS 開啟 <code>AzurPilot.app</code>；Linux 執行 <code>AzurPilot</code>。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/步驟-03-purple?style=flat-square" alt="Step 3"><br>
        <h4>等待環境初始化</h4>
      </div>
      啟動畫面即時展示進度：建立 <code>.venv</code>、更新儲存庫、同步依賴。首次啟動需連網，之後重複啟動只會聚焦已有視窗。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/步驟-04-green?style=flat-square" alt="Step 4"><br>
        <h4>進入 WebUI</h4>
      </div>
      視窗開啟即已登入 AzurPilot WebUI，直接設定出擊任務與排程規則，開始自動化巡航。
    </td>
  </tr>
</table>

> [!TIP]
> 啟動器支援 `--lang` 參數強制指定介面語言（`zh-CN` / `zh-TW` / `ja` / `en`），以及 `--skip-update` 跳過儲存庫更新、`--preview-crash` 預覽錯誤頁面等除錯參數。

> [!WARNING]
> **macOS 使用者**：程式未經開發者簽署，首次開啟若報錯，請先在終端機執行 `xattr -dr com.apple.quarantine AzurPilot.app` 再啟動。

> [!IMPORTANT]
> **Linux 使用者**：程式依賴 `libwebkit2gtk-4.1` 與較新的 `glibc`（CI 使用 Ubuntu 22.04 建構）。若系統缺少相關函式庫導致啟動器無法執行，AzurPilot 本體通常仍可透過命令列正常使用。

---

## 應用預覽

| 平台 | 简体中文 | English |
|:---:|:---:|:---:|
| **Windows** | <img src="screenshots/win-cn.webp" width="420"/> | <img src="screenshots/win-en.webp" width="420"/> |
| **macOS** | <img src="screenshots/mac-cn.webp" width="420"/> | <img src="screenshots/mac-en.webp" width="420"/> |

---

## 與原版啟動器的差異

相較於 [AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript) 自帶的 Electron 啟動器：

| # | 差異點 | 說明 |
|:---:|:---|:---|
| 1 | **全平台支援** | 不再局限於 Windows，macOS（含 Apple Silicon 原生）與 Linux 均可執行 |
| 2 | **更新策略重構** | 只更新 git 儲存庫，並用 `.venv` 內建的 uv 同步依賴；重複啟動僅重新聚焦已有視窗，不做破壞性清理 |
| 3 | **依賴版本鎖定** | Python 套件版本由 `pyproject.toml` 與 `uv.lock` 鎖定，自動同步預設啟用，杜絕環境漂移 |
| 4 | **adb 策略** | 不再重新啟動與替換 adb |
| 5 | **目錄結構調整** | Python / Python / dependencies / ADB 全部收納進 `.venv`，詳見下方目錄結構 |
| 6 | **Node.js 自動化 (Windows)** | 偵測系統級 Node.js；未偵測到時可在確認後自動下載並安裝 |

---

## 目錄結構

```
AzurPilot 根目錄
* Windows: AzurLaneAutoScript
* macOS:   AzurPilot.app/Contents/AzurLaneAutoScript
* Linux:   AzurLaneAutoScript

AzurPilot 啟動器
* Windows: AzurLaneAutoScript/AzurPilot.exe
* macOS:   AzurPilot.app/Contents/MacOS/AzurPilot
* Linux:   AzurLaneAutoScript/AzurPilot

Python / uv
* 所有系統: .venv

Git
* Unix:    .azurpilot/tools/git/bin/git
* Windows: .azurpilot/tools/git/cmd/git.exe

Adb
* Unix:    .azurpilot/tools/adb/adb
* Windows: .azurpilot/tools/adb/adb.exe

啟動器會附加的環境變數
* Unix:    .venv/bin
* Windows: .venv/Scripts
```

---

## 開發活躍度

<table width="100%">
  <thead>
    <tr>
      <th colspan="3" align="left">
        <img src="https://img.shields.io/badge/Metrics-Repo%20Activity-5C3EE8?style=flat-square&logo=github&logoColor=white" alt="Metrics">
        <b>儲存庫開發活躍度與回應態勢</b>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="33%">
        <b>程式碼提交頻度</b><br>
        <img src="https://img.shields.io/github/commit-activity/m/wess09/AzurPilotLauncher?style=flat-square&color=00d4aa" alt="Commit Activity"><br>
        <small>月度提交活躍狀態</small>
      </td>
      <td width="33%">
        <b>合併請求處理</b><br>
        <img src="https://img.shields.io/github/issues-pr-closed/wess09/AzurPilotLauncher?style=flat-square&color=6f42c1" alt="Closed PRs"><br>
        <small>已歸檔合併請求總計</small>
      </td>
      <td width="33%">
        <b>問題回報解決</b><br>
        <img src="https://img.shields.io/github/issues-closed/wess09/AzurPilotLauncher?style=flat-square&color=28a745" alt="Closed Issues"><br>
        <small>已成功解決 Issue 統計</small>
      </td>
    </tr>
    <tr>
      <td width="33%">
        <b>開發分支</b><br>
        <img src="https://img.shields.io/badge/Branch-main-181717?style=flat-square&logo=git&logoColor=white" alt="Main Branch"><br>
        <small>主幹持續交付流</small>
      </td>
      <td width="33%">
        <b>自動化建構</b><br>
        <img src="https://img.shields.io/badge/CI%2FCD-GitHub%20Actions-2088FF?style=flat-square&logo=githubactions&logoColor=white" alt="CI"><br>
        <small>三平台矩陣持續驗證</small>
      </td>
      <td width="33%">
        <b>最新提交紀錄</b><br>
        <a href="https://github.com/wess09/AzurPilotLauncher/commits/main"><img src="https://img.shields.io/github/last-commit/wess09/AzurPilotLauncher?style=flat-square&color=586069" alt="Last Commit"></a><br>
        <small>追蹤最新程式碼演進</small>
      </td>
    </tr>
  </tbody>
</table>

---

## Star 成長趨勢

採用自適應深淺模式的 Star-History 矢量歷史圖譜，直觀反映專案發展脈絡：

<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date" />
    <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date" width="100%" />
  </picture>
  <br>
  <sub>資料來源基於 <a href="https://star-history.com/#wess09/AzurPilotLauncher&Date">Star-History</a> 即時更新 · 點擊可檢視互動式完整走勢</sub>
</div>

---

## 技術棧與依賴致謝

### 啟動器技術棧

| 依賴元件 | 授權標識 | 職責定位 |
| :--- | :--- | :--- |
| **Tauri 2** | <img src="https://img.shields.io/badge/License-MIT%20%7C%20Apache--2.0-brightgreen?style=flat-square" alt="MIT / Apache-2.0"> | 跨平台桌面殼、WebView 承載與系統整合 |
| **Rust** | <img src="https://img.shields.io/badge/License-MIT%20%7C%20Apache--2.0-brightgreen?style=flat-square" alt="MIT / Apache-2.0"> | 系統程式語言，啟動器全部原生邏輯 |
| **uv** | <img src="https://img.shields.io/badge/License-Apache--2.0%20%7C%20MIT-brightgreen?style=flat-square" alt="Apache-2.0 / MIT"> | 內嵌的 Python 套件管理器，建構可重定位 `.venv` |
| **CPython 3.14.x** | <img src="https://img.shields.io/badge/License-PSF--2.0-blue?style=flat-square" alt="PSF-2.0"> | 自動化引擎的 Python 執行環境 |
| **Git** | <img src="https://img.shields.io/badge/License-GPL--2.0-blue?style=flat-square" alt="GPL-2.0"> | 儲存庫程式碼更新（Unix 源碼編譯 / Windows MinGit） |
| **Android platform-tools** | <img src="https://img.shields.io/badge/License-Apache--2.0-brightgreen?style=flat-square" alt="Apache-2.0"> | adb 除錯橋，模擬器與裝置通訊 |

### 生態上游

| 依賴元件 | 授權標識 | 職責定位 |
| :--- | :--- | :--- |
| **AzurPilot** | <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"> | 核心自動化引擎本體（本啟動器的服務對象） |
| **AzurLaneAutoScript** | <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"> | 碧藍航線自動化的上游奠基專案 |

---

## 社群生態與支援

<table width="100%">
  <tr>
    <td width="50%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher/graphs/contributors">
          <img src="https://img.shields.io/badge/Community-Contributors-blueviolet?style=for-the-badge&logo=github&logoColor=white" alt="Contributors">
        </a>
        <br><br>
        <h4>開源共建與參與</h4>
        <p>感謝所有參與程式碼提交、架構改進、問題排查與功能驗證的開發者。歡迎隨時提交 Issue 與 PR 參與共建！</p>
        <a href="https://github.com/wess09/AzurPilotLauncher/graphs/contributors">
          <img src="https://img.shields.io/badge/檢視完整貢獻名單-GitHub%20Graph-181717?style=flat-square&logo=github&logoColor=white" alt="View Contributors">
        </a>
        <a href="https://github.com/wess09/AzurPilotLauncher/issues/new/choose">
          <img src="https://img.shields.io/badge/提交回饋-New%20Issue-0052cc?style=flat-square&logo=githubissues&logoColor=white" alt="New Issue">
        </a>
      </div>
    </td>
    <td width="50%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher/stargazers">
          <img src="https://img.shields.io/badge/Project-Star%20History-f5a623?style=for-the-badge&logo=star&logoColor=white" alt="Star History">
        </a>
        <br><br>
        <h4>專案成長與支援</h4>
        <p>如果本啟動器為你的掛機體驗帶來了便利，歡迎前往儲存庫首頁點亮 Star 支援專案演進。</p>
        <a href="https://star-history.com/#wess09/AzurPilotLauncher&Date">
          <img src="https://img.shields.io/badge/Star%20趨勢看板-Star--History-orange?style=flat-square" alt="Star History Link">
        </a>
      </div>
    </td>
  </tr>
</table>

---

## 授權條款

本專案採用 [GNU General Public License v3.0 (GPL-3.0)](LICENSE) 授權開源——因為 AzurPilot 採用 GPLv3，本啟動器與其保持一致。

---

<div align="center">
  <sub>AzurPilot Launcher 遵守開源授權並由社群驅動維護。如遇問題歡迎提交 <a href="https://github.com/wess09/AzurPilotLauncher/issues">Issue</a> 或 <a href="https://github.com/wess09/AzurPilotLauncher/pulls">Pull Request</a>。</sub>
</div>

## 啟動與開發

開啟後先進入首頁，點擊「啟動」才準備環境並開啟 WebUI。首頁提供停止服務與環境修復；修復重建 .venv，保留設定及日誌。啟動器更新需點擊執行。

開發需要 Node.js 24。

```bash
npm --prefix frontend ci
cargo tauri dev
npm --prefix frontend test
npm --prefix frontend run build
cargo build --locked --release --features custom-protocol
```

[React launcher architecture and IPC](docs/launcher-ui.md)
