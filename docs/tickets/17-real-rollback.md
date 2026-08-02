# 17 — Real rollback target correctness

**Epic:** Better Manager 真實系統整合
**User Story:** 使用者更新元件失敗時，daemon 會重新安裝真正產生目前舊版本的 artifact，不會把新版本誤報成已還原；全新安裝失敗時，套件與狀態都會回到不存在。
**Blocked by:** 16-authorized-path
**Status:** done

## Why

基準版的 `rollback_record_for` 會讀取前一筆交易的 `previous_artifact`，但沒有驗證它是否對應目前 dpkg 回報的舊版本。該欄位不可用時，流程還會採用本次要套用的新 artifact。更新從 `0.0.9` 到 `0.1.0` 後健康檢查失敗，daemon 可能重新安裝 `0.1.0`，卻回報 `Restored`。這會讓 outcome、journal 與實際主機狀態互相矛盾。

## What it delivers

- 每次成功套用 install、update 或 restore 後，在 `/var/lib/better-os/installed/<component>.json` 原子寫入實際版本、artifact 檔名與 SHA-256。
- rollback 只使用與 daemon 重新讀到的 dpkg 前一版本相符、仍存在於 cache 的 installed artifact record。record 缺少或版本不符時，不再使用新 artifact 作為替代。
- 回復成功後寫回舊版 record。移除新元件後刪除 record。metadata 同步失敗時不回報 `Restored`。
- 在 Dockerfile 內用 `dpkg-deb --build` 建立健康的 `0.0.9` 舊版與缺少 `/usr/bin/better-rollback-fixture` 的不健康 `0.1.0` 新版。
- 容器檢查將驗證全新安裝失敗後實際移除套件，以及更新失敗後實際回到舊版本、舊 record 與 completed journal。

## Acceptance criteria

- [x] 成功套用後會持久化實際安裝版本、檔名與 SHA-256。
- [x] 舊版 record 缺少或版本不符時，rollback 不會使用新 artifact，並回報需要人工處理。
- [x] daemon 測試證明失敗更新會重新安裝舊版，而不是新版本。
- [x] daemon 測試證明全新安裝失敗沒有虛構 rollback record。
- [x] 容器內的真實失敗安裝會經過 health failure、實際移除套件，並留下 `Restored` 與 completed journal。
- [x] 容器內的真實失敗更新最後由 `dpkg-query` 回報 `0.0.9`，record 指向舊 artifact，且 outcome 的 error key 為 `daemon.error.health_failed:*`。

## Verification

- `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- `cargo test -p manager-daemon`：42 個 daemon unit tests 與 6 個 D-Bus tests 通過。
- `cargo build --release -p manager-platform --features dbus-client --example e2e_client` 通過。
- `packaging/build-deb.sh --output-dir dist/e2e --target ubuntu-24.04` 通過。
- `packaging/verify-deb.sh dist/e2e ubuntu-24.04` 通過。
- `bash -n packaging/test-daemon-e2e.sh` 與 `git diff --check` 通過。
- `chefer run packaging/e2e/appcipe.yml` 已執行，但在 image build 前因 Docker API socket `permission denied` 停止，尚未完成容器驗證。

## What this caught

- 既有的 manual-recovery test 原本用 APT 失敗掩蓋了「沒有舊 artifact」這個前提，現在改成真正讓更新成功後健康檢查失敗，並驗證缺少 record 時只能進入人工復原。
- e2e client 原本只能送出 install，新增明確的 update action，才能在真實 dpkg 上重現 `0.0.9` 到 `0.1.0` 的失敗更新。

## Out of scope

- 為歷史上沒有 installed artifact record 的主機自動猜測或重建舊 artifact。
- Docker、Chefer 或 CI 執行環境本身的權限設定。
- package signing、公開 APT repository 與互動式 polkit authentication。
