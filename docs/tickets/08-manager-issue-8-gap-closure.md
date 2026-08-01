# 08 — Issue #8 剩餘缺口收斂

**Epic:** Better Manager
**User Story:** 使用者在任何元件上都能看到它的用途、圖示、取代或強化對象與重啟需求，
且介面預設為深色。
**Blocked by:** 07-manager-functional-acceptance
**Status:** done

## What it delivers

- manifest 新增 `summary`、`icon`、`restart` 三個欄位。`summary` 上限 120 字元，
  `icon` 為封閉集合，`restart` 未宣告時維持「未宣告」。
- `manager-core` 的 `RestartRequirement` 由單一 `NotDeclared` 擴為五種狀態，
  `PlanStep` 帶入 `replaces` 與 `enhances`，交易層取所有步驟中最寬的中斷需求。
- 新增 `manager-platform`，收攏系統能力、下載、套件與特權執行介面。
  所有出貨實作皆為 mock，且沒有任何路徑會真的套用套件變更。
- GUI 改由 manifest 驅動名稱、用途與圖示，不再以寫死的三個 id 比對，
  因此不在名單上的元件不會再被整個濾掉。
- 深色為預設外觀，另提供淺色與跟隨系統，設定隨其他設定一起持久化。

## Acceptance criteria

- [x] 元件列與詳情顯示 manifest 宣告的用途與圖示；未宣告時顯示「未宣告」而非猜測。
- [x] 元件詳情與 review 畫面顯示 `replaces` 與 `enhances`。
- [x] review 畫面顯示每個步驟與整筆交易的重啟需求；未宣告不會被降級為「不需重啟」。
- [x] 沒有翻譯的第三方元件仍以自身 manifest 呈現並可操作。
- [x] `manager-platform` 存在，`manager-core` 透過它取得主機 profile。
- [x] 出貨的 package backend 與特權執行器一律拒絕請求。
- [x] 首次啟動為深色；設定頁可切換深色／淺色／系統；舊 state 檔載入為深色。
- [x] `cargo fmt`、workspace check/test、clippy `-D warnings` 全數通過。

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo test --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- CLI lifecycle smoke 與 `ZED_HEADLESS=1` GUI 啟動 smoke

## Out of scope

- 真實下載、APT、特權 daemon、IPC 協定。
- manifest 宣告文字的在地化。
- 最終色票、強調色與高對比無障礙主題。
