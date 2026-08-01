# 13 — GUI 真實執行與非同步進度

**Epic:** Better Manager 真實系統整合
**User Story:** 使用者在圖形介面按下安裝後，看得到真實的下載與安裝進度，
知道什麼時候還能取消，失敗時看得懂發生了什麼。
**Blocked by:** 12-real-platform-backends
**Status:** todo

## What it delivers

- 執行模式的區分：真實模式在背景執行緒跑 `TransactionRunner`，由 runner 單一
  寫入狀態，事件透過 channel 回到前景更新畫面。UI 執行緒不做網路或 IPC。
- 下載進度顯示實際位元組數；階段變化來自 runner 事件而非使用者點擊。
- 取消規則：下載階段可取消並還原開始前快照；進入安裝階段後取消鈕停用，
  因為那時已無法保證還原。`docs/manager-ux-logic.md` 同步更新。
- Mock 模式保留現有的逐步「繼續」流程，並以可見的 demo 標示區隔。
- 新的失敗證據鍵補上雙語文案：下載網路錯誤與雜湊不符、daemon 不可用、
  polkit 拒絕、dpkg 失敗、健康檢查版本不符、還原缺件。

## Acceptance criteria

- [ ] 真實安裝期間介面不卡頓，進度會更新。
- [ ] 下載中可取消並回到開始前狀態；安裝開始後取消鈕停用。
- [ ] 每個新證據鍵在英文與正體中文都有文案，未知鍵仍走既有的保底顯示。
- [ ] Mock demo 模式行為與現在一致，且畫面明確標示是 demo。
- [ ] 操作進行中不會與設定寫入互相覆蓋。
- [ ] `cargo fmt`、workspace check/test、clippy `-D warnings` 全數通過。

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo test --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- `ZED_HEADLESS=1` GUI 啟動 smoke
- Chefer 容器內以真實 daemon 手動走一次安裝、取消與失敗畫面

## Out of scope

- 封裝與預設模式切換（票 14）。
