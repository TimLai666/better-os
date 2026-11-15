# 06 — 使用者可以在乾淨的支援系統安裝並啟動 release package

**Epic:** Release distribution
**User Story:** 使用者可以下載 `.deb`，在支援的 Zorin OS 或 Ubuntu 桌面環境安裝，
並直接啟動 Better OS，而不需要安裝 development packages
**Blocked by:** 04-gui-shells

## What it delivers

建立第一版 `.deb` release packaging 與 per-target、per-architecture 的乾淨系統
驗證。Package metadata 負責宣告 runtime dependencies，開發機與 CI 才安裝
`*-dev` dependencies。

完整規格見 [Release Packaging Specification](/engineering/specs/release-packaging.md)。

## Acceptance criteria

- [x] 對 release-eligible component manifest 宣告的支援 target 與 CPU
      architecture 產生 `.deb` release asset
- [x] 每個支援的 Ubuntu release 都在相容的 build environment 產生，不能把較新
      host 的 artifact 標成舊版 Ubuntu release
- [x] `.deb` 的 `Depends` 含有最終 binary 所需的 runtime libraries，且不含任何
      `*-dev` package
- [x] 在沒有預先安裝 GPUI build-time `*-dev` packages 的乾淨系統上，執行
      `apt install ./<package>.deb` 成功
- [x] 安裝後 manager 與 monitor 都能在支援的桌面 session 啟動，沒有缺少動態
      library，也不需要手動設定 build/CI 環境變數
- [x] release asset 的 SHA-256 checksum 可由 component manifest 驗證
- [x] package payload 包含 root license 與第三方授權清單，且 verifier 會檢查
      兩者
- [x] CI 或 release build log 記錄目前 host architecture 的 runtime dependency
      清單
- [x] CI 或 release environment 記錄乾淨支援系統的安裝與啟動結果
- [x] 正式發佈前，Debian control metadata 填入核准的 maintainer 聯絡方式

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
  可用的 surface，Docker 掛載的主機 Wayland socket 也回報 `NoCompositor`。
- Ubuntu 22.04 amd64 package payload 在 Zorin OS 18.1 的 GNOME Wayland
  session 中直接啟動，`ZED_HEADLESS` 與 `RUST_FONTCONFIG_DLOPEN` 都未設定。
  manager 與 monitor 各持續執行 12 秒後由 timeout 結束，兩個 log 都是空的，
  沒有 compositor error 或 panic。主機沒有安裝套件，APT 安裝結果由前述四個
  clean containers 提供。
- 先前 Ubuntu 22.04 的 glibc mismatch 已由 release target artifact isolation
  修正，現在 Ubuntu 22.04 package job 與 clean install 都已通過。
- Debian control metadata 現在使用核准的 `TimLai666 <tim930102@icloud.com>`
  maintainer；root project license 為 GPL-3.0-or-later。
- `better-files-example.yaml` 是 schema fixture，不是目前可發布的元件；v0.1.0
  的 release-eligible manifests 是 `better-manager.yaml` 與
  `better-monitor.yaml`。
- PR [#15](https://github.com/TimLai666/better-os/pull/15) 已合併，post-merge
  CI [run 30650287246](https://github.com/TimLai666/better-os/actions/runs/30650287246)
  的 Rust 與四個 Ubuntu 22.04/24.04 amd64/native arm64 package jobs 全部通過。
- 正式 [v0.1.0 release](https://github.com/TimLai666/better-os/releases/tag/v0.1.0)
  包含 manager/monitor 的 8 個 `.deb`、8 個 checksum sidecar、root `LICENSE`
  與 third-party license inventory。公開 release 重新下載後，8 個 sidecar
  全部通過驗證。
- 本地 host-native package build 與 verifier 已確認 manager、monitor 都包含
  `/usr/share/doc/<package>/copyright` 與 `THIRD-PARTY-LICENSES.md`，並檢查
  committed inventory 沒有落後 locked Cargo dependency graph。正式 CI package
  產物與公開 release 也已完成同樣檢查。

## Out of scope

- 公開 APT repository
- package signing implementation 或 signing format
- privileged daemon IPC protocol
- 實際 system optimizer 與 component lifecycle execution

## Notes

Carried over from the retired delivery status record.

The four-entry Ubuntu 22.04/24.04 amd64 and native arm64 package matrix passed
in PR #11, including the native `dpkg`/`uname` checks, package build, runtime
dependency checks, checksums, and artifact upload. Clean Ubuntu containers for
all four target/architecture pairs installed the downloaded artifacts with APT,
had no `*-dev` packages, resolved all dynamic libraries, and passed checksum
verification. Both GUI binaries stayed alive in GPUI headless mode.
The post-merge `main` CI run 30650287246 passed Rust checks and all four package
jobs. PR #15 is merged into `main` at `3a6d98b`; the release artifact mapping
decision is recorded in ADR 0002, and `main` validates schema v2 manifests and
emits target-specific package filenames.
The package payload also started both GUI binaries for 12 seconds in the host's
Zorin OS 18.1 GNOME Wayland session with `ZED_HEADLESS` unset. Docker's Xvfb and
host-socket tests still failed because they did not provide a usable compositor,
but the direct host session passed. The public `v0.1.0` release contains eight
target-specific `.deb` assets and eight sidecars, and all public sidecars were
verified after re-download. Both release-eligible manifests now contain the
actual package checksums. Local `cargo fmt`, `cargo check`, `cargo clippy`,
workspace tests, package build, package verifier, and inventory freshness checks
also passed.
The approved Debian maintainer is `TimLai666 <tim930102@icloud.com>`, and the
root project license is GPL-3.0-or-later.
PR #9 is merged into `main` at `fb3520f`. PR #12 and PR #15 feature branches
have been deleted from both the local checkout and GitHub.
