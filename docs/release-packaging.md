# Release Packaging Specification

## Goal

支援的 Better OS release 必須能在支援的 Zorin OS 或 Ubuntu 桌面環境安裝，
不要求使用者另外安裝 Rust、編譯器或 GPUI 的 `*-dev` 套件。Release package
本身必須宣告啟動時需要的 runtime dependencies。

## 第一版 release 格式

- 每個版本使用一個 GitHub Release，例如 `v0.1.0`，集中放置所有
  first-party component 的 target-specific `.deb` assets。
- Package asset 命名為
  `<component>_<version>_ubuntu-<release>_<architecture>.deb`，例如
  `better-manager_0.1.0_ubuntu-24.04_amd64.deb`。
- 每個 `.deb` 旁邊發佈對應的 `.deb.sha256` sidecar，供 manifest 的
  `artifacts` variant 驗證。
- 安裝入口是 `apt`，例如
  `sudo apt install ./better-manager_0.1.0_ubuntu-24.04_amd64.deb`。
- 每個支援的 Ubuntu release 必須在相容的 build environment 產生自己的
  artifact。較新的 host 產出的 binary 可能要求較新的 `libc6`，不能直接標成
  舊版 Ubuntu 的 release。
- 公開 APT repository、套件簽章格式與 release channel 仍未決定，不屬於本規格
  的實作範圍。
- 未來若提供 tarball、AppImage 或其他格式，必須另訂等價的 runtime dependency
  與乾淨系統驗證規格。第一版不把它們當成正式安裝路徑。

## Build-time 與 runtime dependencies

| 類型 | 目前可見的套件 | 規範 |
| --- | --- | --- |
| Build-time | `libfontconfig1-dev`、`libxcb1-dev`、`libxkbcommon-dev`、`libxkbcommon-x11-dev` | 只安裝在開發機或 CI，絕不放進 release package 的 `Depends`。 |
| Runtime 範例 | `libfontconfig1`、`libxcb1`、`libxkbcommon0`、`libxkbcommon-x11-0`，以及實際使用的 Wayland runtime libraries | 由每個支援 target、CPU architecture 的最終 binary 與 package metadata 驗證後決定，這份範例不是最終鎖定清單。 |

`RUST_FONTCONFIG_DLOPEN=1` 是 build 與 CI 的設定，不是使用者安裝或啟動
release 時的前置條件。它不會消除桌面程式啟動時對 fontconfig runtime
library 的需求。

Release package 必須符合以下條件：

1. `Depends` 只列出執行時需要的套件，不列任何 `*-dev` 套件。
2. `Depends` 覆蓋最終 binary 實際需要的 X11、Wayland、fontconfig 與其他
   runtime libraries。
3. `apt install ./<package>.deb` 能自動解析並安裝這些 runtime dependencies，
   使用者不需要先照開發文件安裝 linker packages。

## Release 驗證

每個支援的 distribution release 與 CPU architecture 都必須在乾淨的桌面
環境驗證：

- 基礎環境沒有預先安裝 GPUI 建置所需的 `*-dev` 套件。
- `apt install ./<package>.deb` 成功，且 package metadata 沒有把 build-time
  dependencies 帶給使用者。
- 安裝後檢查 binary 的動態 libraries，不能出現缺少的 runtime library。
- 在支援的桌面 session 中啟動 manager 與 monitor，兩者都不需要額外設定
  `RUST_FONTCONFIG_DLOPEN` 或手動安裝 development packages。
- 發佈的 checksum 與 component manifest 中的 checksum 相符。
- 正式發佈前，Debian control metadata 已填入核准的 maintainer 聯絡方式。
- Package payload 必須包含 root project license 與完整的 Cargo third-party
  license inventory，放在 `/usr/share/doc/<package>/`。

以上任一項未通過，就不能把該 target/architecture 標成支援的 release。
最終 runtime dependency 清單與每次驗證結果必須留在 release 的 build log 或
驗證報告中，避免只靠開發機目前安裝的套件推測。

## 與目前專案狀態的關係

repository 現在提供 `packaging/build-deb.sh` 與 `packaging/verify-deb.sh`，可以在
目前 host architecture 產生並檢查帶有 target-specific filename 的八個 `.deb`：
`better-manager`、`better-manager-daemon`、`better-monitor`（視窗、session
service、command line 與 systemd user unit）、`better-launcher`、`better-files`、
`better-touchpad`（含 safe-mode desktop entry）、`better-awake`（service、tray、
settings 視窗、user unit 與兩個 desktop entries）、`better-storage`（service、
doctor、user unit 與 session D-Bus activation file）。套件只安裝 systemd user
unit，不啟用它；啟用是 Better Manager 的 enable 步驟，與 `better-manager-daemon`
的做法一致。CI 以 `ubuntu-22.04-arm64`、`ubuntu-24.04-arm64` 作為 arm64
job 的隔離目錄，但 asset target 仍是 `ubuntu-22.04` 或 `ubuntu-24.04`，因此
兩種架構的檔名都符合同一套 release 命名規則。`better-files-example` 只作為
schema fixture，不列入正式 release matrix。

第一個 release 發布於
[`v0.1.0`](https://github.com/TimLai666/better-os/releases/tag/v0.1.0)，由
merge commit `3a6d98b73b838c5a2c0d94404ae9313844009e56` 的 post-merge CI run
[`30650287246`](https://github.com/TimLai666/better-os/actions/runs/30650287246)
產生，只包含 `better-manager` 與 `better-monitor`。

第二個 release 是
[`v0.2.0`](https://github.com/TimLai666/better-os/releases/tag/v0.2.0)，由 merge
commit `96b46f11e814bd4088f630c9c19727e45af9132f` 的 post-merge CI run
[`33389237001`](https://github.com/TimLai666/better-os/actions/runs/33389237001)
產生，包含全部八個套件，共 32 個 `.deb` 與 32 個 `.deb.sha256`，涵蓋 ubuntu
22.04 與 24.04 的 amd64 與 arm64。`better-monitor` 因為 payload 在 ticket 36
變寬，已隨那次一起升版，同一個版號不再對應兩種內容。

目前的 release 是
[`v0.2.1`](https://github.com/TimLai666/better-os/releases/tag/v0.2.1)，由 merge
commit `8dc9c7eede37917d5af527a0d8df17e84213b48b` 的 post-merge CI run
[`33871736272`](https://github.com/TimLai666/better-os/actions/runs/33871736272)
產生，同樣是八個套件、32 個 `.deb` 與 32 個 `.deb.sha256`。這是 patch release，
唯一變動的 payload 是 `better-touchpad`：它現在會安裝 GNOME Shell adapter
extension `touchpad-adapter@betteros.org` 與 `better-touchpad-gestured` 服務。
兩次 release 都是所有 asset 從公開 release 重新下載、逐一驗證 checksum 之後，
才把數值寫回七份 component manifest。升版當下 manifest 內的 checksum 描述的是
上一個 release 的檔案，因此 release branch 上會先換回 placeholder，等新 release
公開後再寫入真值。

Debian metadata 使用核准的
`TimLai666 <tim930102@icloud.com>`
maintainer，root project license 為 GPL-3.0-or-later。第三方授權清單由
`packaging/generate-third-party-notices.sh` 從 locked Cargo dependency graph
產生，並由 package verifier 檢查套件內的 notice files。
