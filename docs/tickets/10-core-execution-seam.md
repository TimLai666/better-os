# 10 — manager-core 執行縫與狀態 schema v2

**Epic:** Better Manager 真實系統整合
**User Story:** 生命週期的每個階段結果來自實際執行器，而不是呼叫端自己指定的
mock 結果；demo 與測試仍能走完全決定性的路徑。
**Blocked by:** 09-privileged-ipc-contract
**Status:** todo

## What it delivers

- `advance` 改收 `StageOutcome`（Completed / Failed(FailureEvidence) /
  RestoredPartially / RestoreRequiresManualRecovery），保留 `advance_mock` 相容
  shim，讓既有 lifecycle 測試與 demo 行為不變。
- `PlanStep` 帶 `PlanArtifact`（url、sha256、release_asset、expected_bytes）。
  本機路徑不入庫，一律由 sha256 推導，避免 TOCTOU 與多餘的 schema 變動。
- `ComponentSnapshot` 記錄 `artifact_sha256`，讓 Restore 有可驗證的還原目標；
  快取缺件時回報 `manager.error.restore_artifact_missing`，不虛構可還原版本。
- `ManagerState::validate` 改新不變式：非 dry-run 的進行中計畫合法，但每個
  Install/Update/Restore 步驟都必須帶合法 artifact。
- `STATE_SCHEMA_VERSION` 升 2，`manager-store::decode` 就地由 v1 升級。
- 新 `exec` 模組：`StageDriver`、`MockDriver`、`TransactionRunner`、`CancelToken`、
  `RunnerEvent`。CLI 與 GUI 的 mock 路徑改由 runner 驅動，行為維持不變。

## Acceptance criteria

- [ ] 既有 `crates/manager-core/tests/lifecycle.rs` 全數通過，語意未變。
- [ ] MockDriver 與 MockOutcome 的結果逐項對應，有 parity 測試。
- [ ] v1 狀態檔載入後升為 v2；舊版程式讀 v2 走 `UnsupportedSchema` 拒絕路徑，
      不會被誤判為毀損而重設。
- [ ] 非 dry-run 計畫缺 artifact 時 `validate` 拒絕。
- [ ] Restore 缺快取 artifact 時規劃階段即失敗，且不寫入還原點。
- [ ] `cargo fmt`、workspace check/test、clippy `-D warnings` 全數通過。

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo test --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- CLI lifecycle smoke（`--execution mock`）

## Out of scope

- 真實下載、dpkg 對帳、D-Bus client（票 12）。
- GUI 非同步整合（票 13）。
