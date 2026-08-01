# 12 — 真實下載、dpkg 對帳與 D-Bus client

**Epic:** Better Manager 真實系統整合
**User Story:** 元件的 .deb 會真的從發佈頁下載並驗證雜湊，而且管理器知道主機上
實際裝了什麼版本。
**Blocked by:** 10-core-execution-seam, 11-privileged-daemon
**Status:** done

## What it delivers

- `manager-platform::download`：`ArtifactCache`（XDG 快取，檔名即 sha256）與
  `HttpDownloader`（串流雜湊、`.part` 續傳、網路錯誤退避重試；雜湊不符不重試）。
- `manager-platform::dpkg`：`PackageStateProbe` 與唯讀的 `dpkg-query` 實作。
  唯讀查詢不是特權變更，這個界線寫進 ENG.md。
- `dbus-client` feature 下的 `DbusPrivilegedExecutor`，只能由已建立的授權連線建構。
- `manager-core::exec::RealDriver`：Downloading 階段備齊並驗證所有 artifact，
  Installing 階段逐檔 `StageArtifact` 後一次 `ApplyTransaction`，
  其後的階段由 daemon 回傳的報告映射，client 不自行判定成功。
- `Manager::reconcile` 與 `DriftKind`：主機與紀錄不一致時只回報、不改寫紀錄，
  反映到 doctor 與 status，並擋住該元件的規劃直到使用者處理。
- CLI `reconcile` 子命令。

## Acceptance criteria

- [x] 雜湊不符的下載會刪除暫存檔並回報 `download.checksum_mismatch`。
- [x] 中斷的下載可續傳，且續傳後仍以完整雜湊驗證。
- [x] daemon 不可用時回報 `daemon.unavailable`，不會靜默退回 mock。
- [x] 主機漂移（紀錄有、dpkg 沒有／版本不同）會被偵測並阻擋規劃，
      且不改寫 `installed_version`。
- [x] deb 版本的 epoch 與 revision 後綴在比較前被正確剝離，有測試表。
- [x] 下載階段可取消；取消後還原完整開始前快照，安裝開始後不再提供取消。
- [x] real 模式連不上 daemon 時，狀態檔不會留下「已開始」的交易。
- [x] `cargo fmt`、workspace check/test、clippy `-D warnings` 全數通過。

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo test --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- CLI `reconcile` 與 `--execution real` 無 daemon 的錯誤路徑 smoke

## Out of scope

- GUI 的非同步整合與進度畫面（票 13）。
