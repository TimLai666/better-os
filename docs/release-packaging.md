# Release Packaging Specification

## Goal

支援的 Better OS release 必須能在支援的 Zorin OS 或 Ubuntu 桌面環境安裝，
不要求使用者另外安裝 Rust、編譯器或 GPUI 的 `*-dev` 套件。Release package
本身必須宣告啟動時需要的 runtime dependencies。

## 第一版 release 格式

- 每個 first-party component 以 GitHub Release 的 `.deb` asset 發佈。
- 安裝入口是 `apt`，例如 `sudo apt install ./better-manager.deb`。
- `.deb` 與對應的 SHA-256 checksum 一起發佈，供 manifest 驗證。
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

以上任一項未通過，就不能把該 target/architecture 標成支援的 release。
最終 runtime dependency 清單與每次驗證結果必須留在 release 的 build log 或
驗證報告中，避免只靠開發機目前安裝的套件推測。

## 與目前專案狀態的關係

repository 現在提供 `packaging/build-deb.sh` 與 `packaging/verify-deb.sh`，可以在
目前 host architecture 產生並檢查 manager、monitor 的 `.deb`。這兩個腳本不代表
已經完成所有支援 target 的 release。CI 已配置 Ubuntu 22.04 與 24.04 的 matrix，
但要等兩個 runner 都通過，才能把對應 target 標成支援。target-compatible build、
arm64 cross-build、乾淨支援系統安裝與 component manifest checksum 回填，仍由
`docs/tickets/06-release-packaging.md` 追蹤。正式 GitHub Release asset 尚未產生。
