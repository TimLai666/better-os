# 04 — 使用者可以開啟 manager 與 monitor 的 GPUI mock shell

**Epic:** Desktop applications
**User Story:** 使用者可以看到元件管理與系統觀測的第一版桌面導覽
**Blocked by:** 01-workspace-foundation, 02-manager-dry-run, 03-monitor-contracts
**Status:** done

## What it delivers

以 Rust、GPUI、`gpui-component` 建立兩個 GUI binary，使用 `better-ui` 共用
狀態卡、導覽與 mock data；不執行 privileged operation。

## Acceptance criteria

- [x] manager GUI 顯示元件管理畫面與 Update All planning action
- [x] monitor GUI 顯示 GNOME Resources 風格的 broad navigation 與 mock overview
- [x] 兩個 GUI 都只透過 core API 取得資料
- [x] 在具備 Linux desktop linker/runtime libraries 的環境啟動兩個 GUI

## Verification

- `cargo build --workspace` 在已安裝 GPUI Linux linker packages 的環境通過。
- 在 Wayland desktop session 中，`manager-gui` 與 `monitor-gui` 都以未設定
  `RUST_FONTCONFIG_DLOPEN` 的環境啟動並維持執行 8 秒，之後由 timeout 停止。
