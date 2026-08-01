# 07 — Better Manager 功能驗收

**Epic:** Better Manager
**User Story:** 使用者可以在不變更主機系統的前提下，檢視、審查、模擬與復原元件生命週期。
**Blocked by:** 02-manager-dry-run, 04-gui-shells
**Status:** done

## What it delivers

- `manager-core` 以 catalog 驗證過的資料建立可重現的 install、update、enable、
  disable、verify、restore mock lifecycle。
- CLI 與 GPUI 共用同一套 plan、stage、failure evidence 與 recovery outcome。
- `manager-store` 以版本化 JSON 保存狀態、活動與進行中的 mock operation。
- GUI 支援真實 catalog 的總覽、元件、更新、審查、安裝、復原、健康、活動與設定。
- [`manager-ux-logic.md`](../manager-ux-logic.md) 定義每個畫面的狀態、決策、成功出口與失敗處理。

## Acceptance criteria

- [x] 所有 lifecycle action 都透過 `manager-core`，不執行 manifest lifecycle 字串。
- [x] review 顯示元件、變更前後、相依、衝突、路徑、重啟、下載量與還原資訊。
- [x] mock operation 在每一 stage 保存，可在重啟後驗證與續跑。
- [x] failure 顯示失敗 stage 與 evidence，並能回報已還原、部分還原或需人工復原。
- [x] CLI 提供 list、status、validate、plan、update-all、run、continue、restore、
  cancel、doctor 與 activity。
- [x] `en-US`、`zh-TW` 與 system locale 可立即切換；長字串在 100/125/150% policy
  matrix 會換行而不裁切。
- [x] Chefer isolated workspace formatting、check、test、clippy 與 CLI smoke 全數通過。

## Verification

- Disposable Chefer AppCipe passed `cargo fmt --all -- --check`, offline workspace
  check/test, clippy with `-D warnings`, the CLI lifecycle smoke, and the
  8-second `ZED_HEADLESS=1` GUI launch smoke.

## Out of scope

- APT、sudo、下載、root daemon、IPC、真實 health collector、帳號、遙測與自動最佳化。
