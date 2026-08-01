# 11 — 特權 daemon（D-Bus + polkit + APT）

**Epic:** Better Manager 真實系統整合
**User Story:** 經過管理者授權後，系統上真的會安裝、更新、移除或還原第一方元件，
而且每一步都有紀錄、有健康檢查、失敗時會回滾。
**Blocked by:** 09-privileged-ipc-contract
**Status:** todo

## What it delivers

- `manager-daemon` bin crate：zbus 服務 `org.betteros.Manager1`，方法
  `StageArtifact`、`ApplyTransaction`、`GetStatus`、`Cancel`，signals
  `StepProgress`、`TransactionCompleted`，property `ProtocolVersion`。
- `Authorizer` trait 與 polkit 實作，動作 `org.betteros.manager.apply-transaction`；
  測試以 `FakeAuthorizer` 注入。
- 獨立的二次驗證模組：大小與 schema、`better-*` 元件白名單、target release/arch
  自行判讀、檔名與路徑侷限、安裝前重新雜湊與 `dpkg-deb --field` 交叉核對、
  `dpkg-query` 對帳前一版本。daemon 不讀 manifest。
- `AptDriver` trait 與 `apt-get` 實作（清空環境、非互動、lock timeout），
  加上 `FakeAptDriver` 供全失敗路徑測試。
- `/var/lib/better-os` 的交易 journal 與回滾記錄，`/var/cache/better-os/archives`
  的 artifact 快取。回滾記錄只在第一次真正變更該元件前寫入。
- 健康檢查：dpkg 狀態與由套件名推導的執行檔存在性。不執行 manifest 字串。

## Acceptance criteria

- [ ] 變更前失敗不寫回滾記錄；變更後失敗依記錄回滾並回報
      Restored / PartiallyRestored / ManualRecoveryRequired。
- [ ] 同一 transaction id 重送 `ApplyTransaction` 不會重跑，回傳現況。
- [ ] 未授權的呼叫者被拒絕，且錯誤是穩定機器鍵。
- [ ] `StageArtifact` 雜湊不符時檔案不落地。
- [ ] 計畫的 target release/arch 與主機不符時拒絕執行。
- [ ] .deb 的 control 欄位與步驟宣告不符時拒絕安裝。
- [ ] daemon 崩潰後重啟，未完成的 journal 標記為需人工處理，不自動續跑。
- [ ] 私有 session bus 的整合測試在 CI 上以非特權身分通過。

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo test --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- 私有 `dbus-daemon --session` 整合測試（FakeAuthorizer + FakeAptDriver）

## Out of scope

- 封裝、systemd unit 與 polkit 政策檔落地（票 14）。
- 套件簽章與公開 APT repository。
