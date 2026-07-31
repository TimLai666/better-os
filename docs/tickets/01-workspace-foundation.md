# 01 — 開發者可以建立並驗證 Better OS workspace 基礎

**Epic:** 專案基礎
**User Story:** 開發者可以在固定的 workspace 邊界內開始實作 Better OS
**Blocked by:** 無
**Status:** done

## What it delivers

建立 operating contract、crate 邊界、文件入口與可執行的共享型別驗證基礎。

## Acceptance criteria

- [x] workspace 列出 core、manager、monitor、UI crate
- [x] agent 可由 `CLAUDE.md` 導向 `AGENTS.md`
- [x] `better-core` 可解析與驗證 example manifests
- [x] invalid schema、dependency cycle、conflict 有測試
- [x] 未決策事項被記錄，不被程式碼偷偷固定
