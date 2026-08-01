# 09 — 特權 IPC 協定決策與 wire 契約

**Epic:** Better Manager 真實系統整合
**User Story:** 維護者可以在任何實作開始前，讀到特權邊界要用什麼協定、為什麼，
以及雙方必須共用的訊息格式。
**Blocked by:** 08-manager-issue-8-gap-closure
**Status:** done

## What it delivers

- ADR 0007 記錄特權 daemon 的 IPC 決策：D-Bus system bus + polkit、zbus 實作、
  JSON-in-string 承載、由非特權端下載並由 daemon 重新驗證、systemd 硬化取捨、
  以及被接受的殘餘風險。ADR 0005 的 Deferred 段落同步指向 0007。
- 新增 `manager-ipc` crate，作為 wire 契約的唯一真源。client 與 daemon 都只依賴它，
  它不依賴 `manager-core`，因此 daemon 不會沿用規劃端的信任假設。
- 封閉的 `WireAction`（Install/Update/Remove/Restore）、`WirePlan`、`WireStep`、
  `WireArtifact`、`StepReport`、`TransactionOutcome`、`RollbackRecord`、`WireRecovery`，
  全部 `deny_unknown_fields`，並帶 `protocol_version`。
- 解析前先擋的大小上限（計畫 1 MiB、32 個步驟、單一 artifact 512 MiB），
  以及穩定機器鍵形式的錯誤字串。

## Acceptance criteria

- [x] ADR 0007 存在，說明選擇 D-Bus + polkit 的理由與三個被否決的替代方案。
- [x] ADR 0005 不再宣稱 IPC 協定未決。
- [x] `manager-ipc` 只依賴 `better-core`、`serde`、`serde_json`、`thiserror`。
- [x] 未知欄位、錯誤協定版本、超量步驟、非法 sha256、帶路徑分隔的檔名都被拒絕。
- [x] `WirePlan::from_json` 在解析前先檢查位元組上限。
- [x] 錯誤以穩定機器鍵呈現，呈現層自行決定在地化文字。
- [x] `cargo fmt`、workspace check/test、clippy `-D warnings` 全數通過。

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo test --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`

## Out of scope

- daemon 本體、polkit 政策檔、systemd unit（票 11）。
- `manager-core` 的執行縫與 schema 變更（票 10）。
- 真實下載與 APT（票 11、12）。
