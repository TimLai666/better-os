# 05 — 維護者可以在 CI 驗證 workspace 與未決策邊界

**Epic:** 專案治理
**User Story:** 維護者可以用自動化檢查確認程式、文件與安全邊界沒有偏離
**Blocked by:** 01-workspace-foundation
**Status:** done

## What it delivers

建立 workspace CI、架構與政策文件、manifest 文件、回滾安全邊界與決策紀錄。

## Acceptance criteria

- [x] CI 執行 fmt、check、clippy 與 test
- [x] CI 安裝 GPUI Linux linker dependencies
- [x] 文件說明 privileged boundary、rollback 與未決策事項
- [x] 初始 scaffold 沒有新增 LICENSE 或偷偷決定 IPC protocol
