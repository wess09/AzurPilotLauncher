<div align="center">

<img src="icons/icon.png" alt="AzurPilot Launcher Logo" width="200">

# AzurPilot Launcher

**マルチプラットフォーム ネイティブ動作 · Python 環境ゼロ設定 · AzurPilot をワンクリックで更新・起動**

フル機能の『アズールレーン』自動化ツール [AzurPilot](https://github.com/wess09/AzurPilot)（[AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript) ベース）向けのクロスプラットフォーム デスクトップ ランチャー。Tauri 2 + Rust 製で、Windows / macOS / Linux にネイティブ対応しています。

<p align="center">
  <a href="README.md">简体中文</a> |
  <a href="README_en.md">English</a> |
  <b>日本語</b> |
  <a href="README_zh-TW.md">繁體中文</a>
</p>

---

<!-- コア環境およびプラットフォームバッジ -->
<p align="center">
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue.svg?style=flat-square" alt="License: GPL-3.0"></a>
  <a href="https://www.tauri.app/"><img src="https://img.shields.io/badge/Framework-Tauri%202-24C8DB.svg?style=flat-square&logo=tauri&logoColor=white" alt="Framework: Tauri 2"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Language-Rust-DEA584.svg?style=flat-square&logo=rust&logoColor=white" alt="Language: Rust"></a>
  <a href="https://www.python.org/"><img src="https://img.shields.io/badge/Python-3.14.x-3776AB.svg?style=flat-square&logo=python&logoColor=white" alt="Python 3.14.x"></a>
  <a href="https://github.com/astral-sh/uv"><img src="https://img.shields.io/badge/Packaging-uv-DE5FE9.svg?style=flat-square" alt="Packaging: uv"></a>
  <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-181717.svg?style=flat-square&logo=github&logoColor=white" alt="Platform: Windows / macOS / Linux">
</p>

<!-- 技術スタックおよびフレームワークバッジ -->
<p align="center">
  <a href="https://developer.mozilla.org/ja/docs/Web/API/Web_Workers_API"><img src="https://img.shields.io/badge/Shell-WebView%20%2B%20Rust%20Backend-24C8DB.svg?style=flat-square" alt="Shell: WebView + Rust"></a>
  <img src="https://img.shields.io/badge/i18n-4%20Languages-blueviolet.svg?style=flat-square" alt="i18n: 4 Languages">
  <img src="https://img.shields.io/badge/Update-Git%20%2B%20uv%20Sync-brightgreen.svg?style=flat-square" alt="Update: Git + uv Sync">
  <img src="https://img.shields.io/badge/Self%20Update-mTLS%20Secured-orange.svg?style=flat-square" alt="Self Update: mTLS">
  <a href="https://deepwiki.com/wess09/AzurPilotLauncher"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki"></a>
</p>

<!-- リポジトリ指標バッジ -->
<p align="center">
  <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/github/v/release/wess09/AzurPilotLauncher?style=flat-square&color=007ec6&label=Latest%20Release" alt="Latest Release"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/releases"><img src="https://img.shields.io/github/downloads/wess09/AzurPilotLauncher/total?style=flat-square&color=28a745&label=Downloads" alt="Total Downloads"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/stargazers"><img src="https://img.shields.io/github/stars/wess09/AzurPilotLauncher?style=flat-square&color=f5a623&label=Stars" alt="Stars"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/network/members"><img src="https://img.shields.io/github/forks/wess09/AzurPilotLauncher?style=flat-square&color=6f42c1&label=Forks" alt="Forks"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/issues"><img src="https://img.shields.io/github/issues/wess09/AzurPilotLauncher?style=flat-square&color=d73a49&label=Issues" alt="Open Issues"></a>
  <a href="https://github.com/wess09/AzurPilotLauncher/commits/main"><img src="https://img.shields.io/github/last-commit/wess09/AzurPilotLauncher?style=flat-square&color=586069&label=Last%20Commit" alt="Last Commit"></a>
</p>

<p align="center">
  <a href="#プロジェクト概要と開発指標">概要指標</a> •
  <a href="#プロジェクト概要">概要</a> •
  <a href="#関連エコシステムプロジェクト">エコシステム</a> •
  <a href="#主な特長">主な特長</a> •
  <a href="#システムアーキテクチャ全景">アーキテクチャ</a> •
  <a href="#動作環境と互換性">動作環境</a> •
  <a href="#クイックスタート">クイックスタート</a> •
  <a href="#アプリプレビュー">プレビュー</a> •
  <a href="#オリジナルランチャーとの違い">変更点</a> •
  <a href="#ディレクトリ構成">構成</a> •
  <a href="#開発アクティビティ">開発状況</a> •
  <a href="#star-の推移">スター推移</a> •
  <a href="#コミュニティとサポート">コミュニティ</a>
</p>

</div>

---

## プロジェクト概要と開発指標

<table width="100%">
  <thead>
    <tr>
      <th colspan="4" align="left">
        <img src="https://img.shields.io/badge/Project%20Overview-AzurPilot%20Launcher-181717?style=flat-square&logo=github&logoColor=white" alt="Overview">
        <b>主要指標・開発状況</b>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="25%"><b>最新リリース</b><br><a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/github/v/release/wess09/AzurPilotLauncher?style=flat-square&color=007ec6" alt="Release"></a></td>
      <td width="25%"><b>累計ダウンロード数</b><br><a href="https://github.com/wess09/AzurPilotLauncher/releases"><img src="https://img.shields.io/github/downloads/wess09/AzurPilotLauncher/total?style=flat-square&color=28a745" alt="Downloads"></a></td>
      <td width="25%"><b>オープンソースライセンス</b><br><a href="./LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"></a></td>
      <td width="25%"><b>自動ビルド</b><br><a href="https://github.com/wess09/AzurPilotLauncher/actions"><img src="https://img.shields.io/badge/CI-GitHub%20Actions-2088FF?style=flat-square&logo=githubactions&logoColor=white" alt="CI Status"></a></td>
    </tr>
    <tr>
      <td width="25%"><b>リポジトリサイズ</b><br><img src="https://img.shields.io/github/repo-size/wess09/AzurPilotLauncher?style=flat-square&color=586069" alt="Repo Size"></td>
      <td width="25%"><b>開発言語</b><br><img src="https://img.shields.io/badge/Language-Rust%20%7C%20Tauri-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Language"></td>
      <td width="25%"><b>ランタイム</b><br><img src="https://img.shields.io/badge/Python-3.14.x%20%2B%20uv-3776AB?style=flat-square&logo=python&logoColor=white" alt="Python Runtime"></td>
      <td width="25%"><b>UI 言語</b><br><img src="https://img.shields.io/badge/i18n-%E7%AE%80%E4%BD%93%20%7C%20%E7%B9%81%E9%AB%94%20%7C%20%E6%97%A5%E6%9C%AC%E8%AA%9E%20%7C%20En-blueviolet?style=flat-square" alt="i18n"></td>
    </tr>
    <tr>
      <td colspan="4">
        <b>クイックアクション：</b>
        <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest"><img src="https://img.shields.io/badge/Release-最新版をダウンロード-0052cc?style=flat-square&logo=github&logoColor=white" alt="Download"></a>
        <a href="https://deepwiki.com/wess09/AzurPilotLauncher"><img src="https://img.shields.io/badge/Wiki-Ask%20DeepWiki-6366F1?style=flat-square" alt="Ask DeepWiki"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/issues/new/choose"><img src="https://img.shields.io/badge/Issue-バグ報告・要望-d73a49?style=flat-square&logo=githubissues&logoColor=white" alt="New Issue"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/pulls"><img src="https://img.shields.io/badge/PR-コードコントリビュート-28a745?style=flat-square&logo=git&logoColor=white" alt="Pull Request"></a>
        <a href="https://github.com/wess09/AzurPilotLauncher/stargazers"><img src="https://img.shields.io/badge/Star-プロジェクトを応援-f5a623?style=flat-square&logo=github&logoColor=white" alt="Star"></a>
      </td>
    </tr>
  </tbody>
</table>

---

## プロジェクト概要

**AzurPilot Launcher** は、フル機能の『アズールレーン』自動化アシスタント **AzurPilot** を Windows・macOS・Linux 上でネイティブにワンクリック起動できるようにするプロジェクトです。Apple Silicon の Mac Mini でも、従来の x86 マシンでも、トランスレータも Docker も不要で、システム環境を汚しません。

ランチャーは単一の `stable.json` を通じて Git/ADB バイナリ、deploy スクリプト、設定テンプレートを取得します。内蔵 uv が最新の安定版 Python 3.14 パッチと依存関係をローカルで導入し、deploy スクリプトがリポジトリ更新と WebUI ビルドを行います。不足時は「デプロイ」を表示します。システム言語が簡体字中国語で北京時間の場合は CNB、それ以外は GitHub を優先し、設定で取得元を選択できます。Git、uv、Python の事前インストールは不要です。[コンポーネント仕様](docs/runtime-components.md)を参照してください。

---

## 関連エコシステムプロジェクト

本プロジェクトは、以下のエコシステム上の主要プロジェクトと密接に連携しています：

<table width="100%">
  <tr>
    <td width="33%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher">
          <img src="https://img.shields.io/badge/Repository-AzurPilot%20Launcher-181717?style=for-the-badge&logo=github&logoColor=white" alt="AzurPilot Launcher">
        </a>
      </div>
      <br>
      <b>クロスプラットフォーム ランチャー (本プロジェクト)</b>
      <p>Windows / macOS / Linux 向けのオールインワン デスクトップ ランチャー。環境構築・コード更新・依存関係の同期・WebUI のライフサイクル管理を一元的に担います。</p>
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
      <b>自動化エンジン本体</b>
      <p>『アズールレーン』自動化アシスタントのコア。コンピュータビジョンによる画像認識、出撃スケジューリング、ローカル Web コンソールサービスを備えます。</p>
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
      <b>上流自動化スクリプト (AzurLaneAutoScript)</b>
      <p>AzurLaneAutoScript の上流プロジェクト。アズールレーン自動化の礎であり、このエコシステムの全自動化機能はここから発展しました。</p>
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

## 主な特長

<table width="100%">
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/モジュール-デプロイ形態-0052cc?style=flat-square" alt="Tag"><br>
      <h3>ゼロ設定の環境構築</h3>
      <p>CI は Git/ADB の配置ツールをパッケージ化します。ランチャーが不足した引導ファイルを取得し、内蔵 uv で Python と依存関係を導入して、既存環境とキャッシュを再利用します。</p>
      <div>
        <img src="https://img.shields.io/badge/ランタイム-Python%203.14.x-orange?style=flat-square" alt="Python">
        <img src="https://img.shields.io/badge/依存管理-uv%20内蔵-blueviolet?style=flat-square" alt="uv">
        <img src="https://img.shields.io/badge/体験-すぐ使える-brightgreen?style=flat-square" alt="Ready">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/モジュール-ネイティブ体験-00875a?style=flat-square" alt="Tag"><br>
      <h3>ネイティブ デスクトップシェル</h3>
      <p>Tauri 2 ベースのネイティブ WebView シェル：グラスモーフィズムのスプラッシュ画面とカスタムタイトルバー、システムトレイ、Windows Toast / macOS / Linux のネイティブ通知、シングルインスタンス動作。再起動時は既存ウィンドウへのフォーカスだけを行います。</p>
      <div>
        <img src="https://img.shields.io/badge/フレームワーク-Tauri%202-blue?style=flat-square" alt="Tauri">
        <img src="https://img.shields.io/badge/統合-トレイ%20%2B%20通知%20%2B%20タイトルバー-teal?style=flat-square" alt="Native">
        <img src="https://img.shields.io/badge/インスタンス-シングル-lightgrey?style=flat-square" alt="Single Instance">
      </div>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/モジュール-更新体系-403294?style=flat-square" alt="Tag"><br>
      <h3>自動更新システム</h3>
      <p>単一の stable.json がランチャーと実行コンポーネントを管理します。ランチャーには CNB と GitHub の取得 URL を提供します。</p>
      <div>
        <img src="https://img.shields.io/badge/コード更新-Git%20リトライ-purple?style=flat-square" alt="Git Update">
        <img src="https://img.shields.io/badge/バージョン固定-uv.lock-blue?style=flat-square" alt="Lock">
        <img src="https://img.shields.io/badge/セルフ更新-mTLS%20マルチミラー-orange?style=flat-square" alt="mTLS">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/モジュール-国際化-172b4d?style=flat-square" alt="Tag"><br>
      <h3>4 言語対応の国際化 UI</h3>
      <p>簡体字中国語・繁体字中国語・日本語・English の 4 言語を内蔵。システムのロケールから自動選択され、<code>--lang</code> 起動オプションでの強制指定にも対応します。WebUI と Web 通知は選択された言語で表示されます。</p>
      <div>
        <img src="https://img.shields.io/badge/言語-简体中文-red?style=flat-square" alt="zh-CN">
        <img src="https://img.shields.io/badge/語言-繁體中文-blue?style=flat-square" alt="zh-TW">
        <img src="https://img.shields.io/badge/言語-日本語-pink?style=flat-square" alt="ja">
        <img src="https://img.shields.io/badge/Language-English-green?style=flat-square" alt="en">
      </div>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/モジュール-ネットワーク戦略-de350b?style=flat-square" alt="Tag"><br>
      <h3>デュアルミラーと直結ポリシー</h3>
      <p>簡体字中国語と北京時間のシステムは CNB、それ以外は GitHub を優先します。設定ではコンポーネント、Python、PyPI のミラー名を選択できます。</p>
      <div>
        <img src="https://img.shields.io/badge/デプロイ-国際版%20%2F%20CN%20ミラー-red?style=flat-square" alt="Mirrors">
        <img src="https://img.shields.io/badge/高速化-npmmirror-red?style=flat-square" alt="npmmirror">
        <img src="https://img.shields.io/badge/接続-スマート直結-green?style=flat-square" alt="Direct">
      </div>
    </td>
    <td width="50%" valign="top">
      <img src="https://img.shields.io/badge/モジュール-ライフサイクル-ff5630?style=flat-square" alt="Tag"><br>
      <h3>堅牢なプロセス・ウィンドウ管理</h3>
      <p>バックエンド起動前にポートを占有するプロセスを自動クリーンアップ。子プロセスは環境変数で所有者をマークし、終了時に全プロセステーブルを走査して回収するため、<code>gui.py</code> がリークしません。WebUI はワンタイムトークンによる事前認証で開き、ウィンドウを開いた瞬間からログイン済みです。</p>
      <div>
        <img src="https://img.shields.io/badge/ポート-自動クリーンアップ-blue?style=flat-square" alt="Port Clean">
        <img src="https://img.shields.io/badge/プロセス-ゼロリーク-brightgreen?style=flat-square" alt="Leak Free">
        <img src="https://img.shields.io/badge/WebUI-認証不要直結-009688?style=flat-square" alt="Auth Free">
      </div>
    </td>
  </tr>
</table>

---

## システムアーキテクチャ全景

プロジェクトはレイヤーごとに明確に分離された設計を採用し、各層の責務と通信境界が明確に定義されています：

```mermaid
graph TD
    subgraph Shell ["ランチャーシェル (Tauri 2 · Rust)"]
        Splash["スプラッシュ画面 React launcher<br/>(グラス調プログレスバー / エラーページ React connection error)"]
        Setup["環境構築 setup.rs<br/>(Python / uv / adb / git の導入と移行)"]
        Tray["システム統合<br/>(システムトレイ / ネイティブ通知 / カスタムタイトルバー)"]
        Backend["バックエンド管理 backend.rs<br/>(ポートクリーンアップ / プロセス回収)"]
    end

    subgraph Venv [".venv ランタイム (uv 管理 · 再配置可能)"]
        PYTHON["CPython 3.14.x インタプリタ"]
        TOOLS["内蔵 Python / dependencies / ADB ツールチェーン"]
        SYNC["uv sync --frozen 依存同期 (uv.lock 固定)"]
    end

    subgraph Repo ["AzurPilot バックエンドリポジトリ"]
        GIT["Git 自動更新 (deploy.git.GitManager · 20 回リトライ)"]
        GUI["gui.py WebUI バックエンド"]
        WEB["WebUI コンソール (127.0.0.1:22267)"]
    end

    Splash --> Setup
    Setup --> TOOLS
    TOOLS --> PYTHON
    Setup --> GIT
    GIT --> SYNC
    SYNC --> GUI
    Backend --> GUI
    GUI --> WEB
    Shell -. "WebView ホスティング · SSE 通知ストリーム" .-> WEB
    Tray -. "デスクトップ通知 / トレイ操作" .-> Shell
```

---

## 動作環境と互換性

<table width="100%">
  <thead>
    <tr>
      <th width="20%">項目</th>
      <th width="40%">要件</th>
      <th width="40%">備考</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td><b>オペレーティングシステム</b></td>
      <td><img src="https://img.shields.io/badge/Windows-10%2B%20(x64%20%2F%20ARM64)-0078D6?style=flat-square&logo=windows&logoColor=white" alt="Windows"> <img src="https://img.shields.io/badge/macOS-ネイティブ対応-000000?style=flat-square&logo=apple&logoColor=white" alt="macOS"> <img src="https://img.shields.io/badge/Linux-x86__64%20%2F%20ARM64-FCC624?style=flat-square&logo=linux&logoColor=black" alt="Linux"></td>
      <td>CI は Ubuntu 22.04 / macOS / 最新 Windows のマトリクスでビルド</td>
    </tr>
    <tr>
      <td><b>WebView ランタイム</b></td>
      <td><img src="https://img.shields.io/badge/Windows-WebView2-0078D6?style=flat-square" alt="WebView2"> <img src="https://img.shields.io/badge/Linux-libwebkit2gtk--4.1-FCC624?style=flat-square" alt="libwebkit2gtk"> <img src="https://img.shields.io/badge/macOS-システム内蔵-000000?style=flat-square" alt="WKWebView"></td>
      <td>Windows 7/8/10 は事前に <a href="https://developer.microsoft.com/zh-cn/microsoft-edge/webview2">WebView2</a> のインストールが必要。Linux は比較的新しい glibc が必要</td>
    </tr>
    <tr>
      <td><b>Python / uv / Git / adb</b></td>
      <td><img src="https://img.shields.io/badge/事前インストール-不要-brightgreen?style=flat-square" alt="No Preinstall"></td>
      <td>ランチャーが uv を内蔵して <code>.venv</code> を自動作成し、git・adb も仮想環境へデプロイ</td>
    </tr>
    <tr>
      <td><b>ネットワーク</b></td>
      <td><img src="https://img.shields.io/badge/初回起動-オンライン必須-yellow?style=flat-square" alt="Network"></td>
      <td>Python のダウンロード、依存関係の同期、リポジトリ更新に必要。CN 版は中国国内ミラーで高速化</td>
    </tr>
    <tr>
      <td><b>Node.js (Windows のみ)</b></td>
      <td><img src="https://img.shields.io/badge/検出-自動処理-orange?style=flat-square&logo=nodedotjs&logoColor=white" alt="Node.js"></td>
      <td>システムレベルの Node.js が見つからない場合、確認後に自動ダウンロード・インストール（SHA-256 検証付き）</td>
    </tr>
  </tbody>
</table>

---

## クイックスタート

<table width="100%">
  <tr>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/ステップ-01-blue?style=flat-square" alt="Step 1"><br>
        <h4>アーカイブを入手</h4>
      </div>
      <a href="https://github.com/wess09/AzurPilotLauncher/releases/latest">Releases の最新版</a> から、<b>お使いの OS と CPU に合ったアーカイブ</b>（国際版または CN ミラー版）をダウンロードします。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/ステップ-02-indigo?style=flat-square" alt="Step 2"><br>
        <h4>解凍して起動</h4>
      </div>
      Windows は <code>AzurPilot.exe</code> を実行（管理者権限が必要）。macOS は <code>AzurPilot.app</code> を開く。Linux は <code>AzurPilot</code> を実行します。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/ステップ-03-purple?style=flat-square" alt="Step 3"><br>
        <h4>環境初期化を待機</h4>
      </div>
      スプラッシュ画面に進行状況がリアルタイム表示されます：<code>.venv</code> の作成、リポジトリの更新、依存関係の同期。初回起動はネット接続が必要で、2 回目以降は既存ウィンドウへのフォーカスのみです。
    </td>
    <td width="25%" valign="top">
      <div align="center">
        <img src="https://img.shields.io/badge/ステップ-04-green?style=flat-square" alt="Step 4"><br>
        <h4>WebUI へアクセス</h4>
      </div>
      ウィンドウを開くと AzurPilot WebUI にログイン済み。出撃タスクとスケジュールを設定して、自動化クルージングを開始しましょう。
    </td>
  </tr>
</table>

> [!TIP]
> ランチャーはコマンドラインオプションに対応しています：`--lang` で UI 言語を強制指定（`zh-CN` / `zh-TW` / `ja` / `en`）、`--skip-update` でリポジトリ更新をスキップ、`--preview-crash` でエラーページをプレビューなど。

> [!WARNING]
> **macOS ユーザーへ**：本アプリは開発者署名されていません。初回起動時にエラーが出る場合は、ターミナルで `xattr -dr com.apple.quarantine AzurPilot.app` を実行してから起動してください。

> [!IMPORTANT]
> **Linux ユーザーへ**：`libwebkit2gtk-4.1` と比較的新しい `glibc` に依存します（CI は Ubuntu 22.04 でビルド）。システムライブラリ不足でランチャーが起動しない場合でも、AzurPilot 本体は通常どおりコマンドラインから利用できます。

---

## アプリプレビュー

| プラットフォーム | 简体中文 | English |
|:---:|:---:|:---:|
| **Windows** | <img src="screenshots/win-cn.webp" width="420"/> | <img src="screenshots/win-en.webp" width="420"/> |
| **macOS** | <img src="screenshots/mac-cn.webp" width="420"/> | <img src="screenshots/mac-en.webp" width="420"/> |

---

## オリジナルランチャーとの違い

[AzurLaneAutoScript](https://github.com/LmeSzinc/AzurLaneAutoScript) 同梱の Electron ランチャーと比較して：

| # | 変更点 | 説明 |
|:---:|:---|:---|
| 1 | **マルチプラットフォーム対応** | Windows 限定ではなく、macOS（Apple Silicon ネイティブ含む）と Linux でも動作 |
| 2 | **更新ポリシーの再構築** | git リポジトリの更新のみを行い、依存関係は `.venv` 内蔵の uv で同期。再起動時は既存ウィンドウへのフォーカスのみで、破壊的なクリーンアップは行いません |
| 3 | **依存バージョンの固定** | Python パッケージのバージョンは `pyproject.toml` と `uv.lock` で固定され、自動同期が既定で有効。環境のドリフトを防止します |
| 4 | **adb ポリシー** | adb の再起動・置き換えは行いません |
| 5 | **ディレクトリ構成の変更** | Python / Python / dependencies / ADB はすべて `.venv` 内に収められます。詳細は下記の構成を参照 |
| 6 | **Node.js 自動化 (Windows)** | システムレベルの Node.js を検出し、見つからない場合は確認後に自動ダウンロード・インストール |

---

## ディレクトリ構成

```
AzurPilot ルートディレクトリ
* Windows: AzurPilot
* macOS:   AzurPilot.app/Contents/AzurPilot
* Linux:   AzurPilot

AzurPilot ランチャー
* Windows: AzurPilot/AzurPilot.exe
* macOS:   AzurPilot.app/Contents/MacOS/AzurPilot
* Linux:   AzurPilot/AzurPilot

Python / uv
* 全システム共通: .venv

Git
* Unix:    .azurpilot/tools/git/bin/git
* Windows: .azurpilot/tools/git/cmd/git.exe

Adb
* Unix:    .azurpilot/tools/adb/adb
* Windows: .azurpilot/tools/adb/adb.exe

ランチャーが追加する環境変数
* Unix:    .venv/bin
* Windows: .venv/Scripts
```

---

## 開発アクティビティ

<table width="100%">
  <thead>
    <tr>
      <th colspan="3" align="left">
        <img src="https://img.shields.io/badge/Metrics-Repo%20Activity-5C3EE8?style=flat-square&logo=github&logoColor=white" alt="Metrics">
        <b>リポジトリ開発アクティビティと対応状況</b>
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="33%">
        <b>コミット頻度</b><br>
        <img src="https://img.shields.io/github/commit-activity/m/wess09/AzurPilotLauncher?style=flat-square&color=00d4aa" alt="Commit Activity"><br>
        <small>月次コミット状況</small>
      </td>
      <td width="33%">
        <b>プルリクエスト対応</b><br>
        <img src="https://img.shields.io/github/issues-pr-closed/wess09/AzurPilotLauncher?style=flat-square&color=6f42c1" alt="Closed PRs"><br>
        <small>クローズ済み PR の累計</small>
      </td>
      <td width="33%">
        <b>Issue 解決状況</b><br>
        <img src="https://img.shields.io/github/issues-closed/wess09/AzurPilotLauncher?style=flat-square&color=28a745" alt="Closed Issues"><br>
        <small>解決済み Issue の統計</small>
      </td>
    </tr>
    <tr>
      <td width="33%">
        <b>開発ブランチ</b><br>
        <img src="https://img.shields.io/badge/Branch-main-181717?style=flat-square&logo=git&logoColor=white" alt="Main Branch"><br>
        <small>メインブランチ継続デリバリー</small>
      </td>
      <td width="33%">
        <b>自動ビルド</b><br>
        <img src="https://img.shields.io/badge/CI%2FCD-GitHub%20Actions-2088FF?style=flat-square&logo=githubactions&logoColor=white" alt="CI"><br>
        <small>3 プラットフォームマトリクスで継続検証</small>
      </td>
      <td width="33%">
        <b>最新コミット</b><br>
        <a href="https://github.com/wess09/AzurPilotLauncher/commits/main"><img src="https://img.shields.io/github/last-commit/wess09/AzurPilotLauncher?style=flat-square&color=586069" alt="Last Commit"></a><br>
        <small>最新のコード進化を追跡</small>
      </td>
    </tr>
  </tbody>
</table>

---

## Star の推移

ダーク/ライトモードに自動適応する Star-History チャートで、プロジェクトの成長の歩みを可視化しています：

<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date" />
    <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=wess09/AzurPilotLauncher&type=Date" width="100%" />
  </picture>
  <br>
  <sub>データは <a href="https://star-history.com/#wess09/AzurPilotLauncher&Date">Star-History</a> よりリアルタイム更新 · クリックでインタラクティブな全体チャートを表示</sub>
</div>

---

## 技術スタックと依存関係の謝辞

### ランチャー技術スタック

| 依存コンポーネント | ライセンス | 役割 |
| :--- | :--- | :--- |
| **Tauri 2** | <img src="https://img.shields.io/badge/License-MIT%20%7C%20Apache--2.0-brightgreen?style=flat-square" alt="MIT / Apache-2.0"> | クロスプラットフォーム デスクトップシェル、WebView ホスティングとシステム統合 |
| **Rust** | <img src="https://img.shields.io/badge/License-MIT%20%7C%20Apache--2.0-brightgreen?style=flat-square" alt="MIT / Apache-2.0"> | システムプログラミング言語。ランチャーの全ネイティブロジック |
| **uv** | <img src="https://img.shields.io/badge/License-Apache--2.0%20%7C%20MIT-brightgreen?style=flat-square" alt="Apache-2.0 / MIT"> | 内蔵の Python パッケージマネージャー。再配置可能な `.venv` を構築 |
| **CPython 3.14.x** | <img src="https://img.shields.io/badge/License-PSF--2.0-blue?style=flat-square" alt="PSF-2.0"> | 自動化エンジンの Python ランタイム |
| **Git** | <img src="https://img.shields.io/badge/License-GPL--2.0-blue?style=flat-square" alt="GPL-2.0"> | リポジトリのコード更新（Unix はソースビルド / Windows は MinGit） |
| **Android platform-tools** | <img src="https://img.shields.io/badge/License-Apache--2.0-brightgreen?style=flat-square" alt="Apache-2.0"> | adb ブリッジ。エミュレータ・デバイスとの通信 |

### エコシステム上流

| 依存コンポーネント | ライセンス | 役割 |
| :--- | :--- | :--- |
| **AzurPilot** | <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"> | 自動化エンジン本体（本ランチャーのサービス対象） |
| **AzurLaneAutoScript** | <img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat-square" alt="GPL-3.0"> | アズールレーン自動化の上流となる基盤プロジェクト |

---

## コミュニティとサポート

<table width="100%">
  <tr>
    <td width="50%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher/graphs/contributors">
          <img src="https://img.shields.io/badge/Community-Contributors-blueviolet?style=for-the-badge&logo=github&logoColor=white" alt="Contributors">
        </a>
        <br><br>
        <h4>オープンソース共創</h4>
        <p>コードのコミット、アーキテクチャの改善、問題の調査、機能の検証にご参加いただいたすべての開発者の皆さんに感謝します。Issue や PR でのご参加をいつでも歓迎します！</p>
        <a href="https://github.com/wess09/AzurPilotLauncher/graphs/contributors">
          <img src="https://img.shields.io/badge/コントリビューター一覧-GitHub%20Graph-181717?style=flat-square&logo=github&logoColor=white" alt="View Contributors">
        </a>
        <a href="https://github.com/wess09/AzurPilotLauncher/issues/new/choose">
          <img src="https://img.shields.io/badge/フィードバック-New%20Issue-0052cc?style=flat-square&logo=githubissues&logoColor=white" alt="New Issue">
        </a>
      </div>
    </td>
    <td width="50%" valign="top">
      <div align="center">
        <a href="https://github.com/wess09/AzurPilotLauncher/stargazers">
          <img src="https://img.shields.io/badge/Project-Star%20History-f5a623?style=for-the-badge&logo=star&logoColor=white" alt="Star History">
        </a>
        <br><br>
        <h4>プロジェクトの成長と支援</h4>
        <p>本ランチャーが自動周回のお役に立ったなら、リポジトリに Star を付けてプロジェクトの発展を応援していただけると嬉しいです。</p>
        <a href="https://star-history.com/#wess09/AzurPilotLauncher&Date">
          <img src="https://img.shields.io/badge/Star%20推移-Star--History-orange?style=flat-square" alt="Star History Link">
        </a>
      </div>
    </td>
  </tr>
</table>

---

## ライセンス

本プロジェクトは [GNU General Public License v3.0 (GPL-3.0)](LICENSE) のもとでオープンソースとして公開されています。AzurPilot が GPLv3 採用のため、本ランチャーも同様に GPLv3 です。

---

<div align="center">
  <sub>AzurPilot Launcher はオープンソースライセンスに基づきコミュニティによってメンテナンスされています。問題があれば <a href="https://github.com/wess09/AzurPilotLauncher/issues">Issue</a> または <a href="https://github.com/wess09/AzurPilotLauncher/pulls">Pull Request</a> をお気軽にどうぞ。</sub>
</div>

## 起動と開発

起動するとホームが表示されます。「起動」を押すと環境を準備して WebUI を開きます。修復前にサービスを停止してください。修復は .venv を再構築し、設定とログを保持します。ランチャー更新はクリックして実行します。

開発には Node.js 24 が必要です。

```bash
npm --prefix frontend ci
cargo tauri dev
npm --prefix frontend test
npm --prefix frontend run build
cargo build --locked --release --features custom-protocol
```

[React launcher architecture and IPC](docs/launcher-ui.md)
