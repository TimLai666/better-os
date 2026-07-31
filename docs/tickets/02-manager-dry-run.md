# 02 — 使用者可以列出元件並產生不會改機的安裝計畫

**Epic:** Better Manager
**User Story:** 使用者可以檢視元件狀態並預覽安裝或更新計畫
**Blocked by:** 01-workspace-foundation
**Status:** done

## What it delivers

CLI 與未來 GUI 共用同一個 manager-core planning API，並以 in-memory backend
證明目前不會執行 privileged mutation。

## Acceptance criteria

- [x] list、status、validate、plan 指令可使用
- [x] plan 支援 install 與 update
- [x] manager-core 有可觀察的 transaction steps 與 execution log
- [x] manager-core 測試證明 dry-run 不會呼叫 host mutation
