# 06 — 使用者可以在乾淨的支援系統安裝並啟動 release package

**Epic:** Release distribution
**User Story:** 使用者可以下載 `.deb`，在支援的 Zorin OS 或 Ubuntu 桌面環境安裝，
並直接啟動 Better OS，而不需要安裝 development packages
**Blocked by:** 04-gui-shells
**Status:** in_progress

## What it delivers

建立第一版 `.deb` release packaging 與 per-target、per-architecture 的乾淨系統
驗證。Package metadata 負責宣告 runtime dependencies，開發機與 CI 才安裝
`*-dev` dependencies。

完整規格見 [Release Packaging Specification](../release-packaging.md)。

## Acceptance criteria

- [ ] 對 component manifest 宣告的支援 target 與 CPU architecture 產生 `.deb`
      release asset
- [x] 每個支援的 Ubuntu release 都在相容的 build environment 產生，不能把較新
      host 的 artifact 標成舊版 Ubuntu release
- [x] `.deb` 的 `Depends` 含有最終 binary 所需的 runtime libraries，且不含任何
      `*-dev` package
- [x] 在沒有預先安裝 GPUI build-time `*-dev` packages 的乾淨系統上，執行
      `apt install ./<package>.deb` 成功
- [ ] 安裝後 manager 與 monitor 都能在支援的桌面 session 啟動，沒有缺少動態
      library，也不需要手動設定 build/CI 環境變數
- [ ] release asset 的 SHA-256 checksum 可由 component manifest 驗證
- [x] CI 或 release build log 記錄目前 host architecture 的 runtime dependency
      清單
- [x] CI 或 release environment 記錄乾淨支援系統的安裝與啟動結果
- [ ] 正式發佈前，Debian control metadata 填入核准的 maintainer 聯絡方式

## Verification so far

- [PR #11](https://github.com/TimLai666/better-os/pull/11) 的 GitHub Actions
  Ubuntu 22.04/24.04 amd64 與 native arm64 package matrix 都通過 build、
  native architecture check、runtime dependency、checksum verification 與
  artifact upload。
- Ubuntu 22.04/24.04 的 amd64 與 arm64 clean containers 都能用 APT 安裝
  manager 與 monitor。四個環境都沒有安裝 `*-dev` package，兩個 binary 的
  `ldd` 都沒有 unresolved library，artifact checksum 也全部通過。
- 四個環境的 manager 與 monitor 都能在 GPUI `ZED_HEADLESS=1` 模式持續執行。
  這是 process smoke，不是支援桌面 session 的啟動證據。Xvfb 沒有提供 GPUI
  可用的 surface，主機 Wayland socket 測試也回報 `NoCompositor`，所以 desktop
  launch acceptance criterion 仍未勾選。
- 先前 Ubuntu 22.04 的 glibc mismatch 已由 release target artifact isolation
  修正，現在 Ubuntu 22.04 package job 與 clean install 都已通過。

## Out of scope

- 公開 APT repository
- package signing implementation 或 signing format
- privileged daemon IPC protocol
- 實際 system optimizer 與 component lifecycle execution
