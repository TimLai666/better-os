# 14 — daemon 封裝、接線與文件重談

**Epic:** Better Manager 真實系統整合
**User Story:** 使用者安裝 Better Manager 後，系統上就有一個可被審查的特權服務，
而且專案文件描述的保證與程式實際行為一致。
**Blocked by:** 13-gui-real-execution

## What it delivers

- 新的 `better-manager-daemon` .deb：`/usr/libexec` 執行檔、D-Bus 啟動的 systemd
  unit、system-services 檔、busconfig、polkit 政策檔。`better-manager` 以
  `Recommends` 指向同版本的 daemon，缺席時前端回報 `daemon.unavailable`。
- 專案第一批 maintainer scripts：postinst 建立狀態與快取目錄並重載設定，
  prerm 停止服務，postrm purge 清理目錄。
- `packaging/verify-deb.sh` 檢查 unit、政策與 busconfig 的落點。
- 執行模式預設切換為真實；mock 保留給測試與 demo，並在 smoke 測試中明寫。
- 不變式重談：`no_shipped_backend_applies_a_package_change` 改為「沒有已核准的
  特權連線就沒有人能套用變更」，以執行期接線表達，不用 cargo feature 分叉。
- 文件同步：AGENTS.md 的禁令與待決清單、`engineering/architecture.md` 測試矩陣、README 的能力描述、
  `engineering/architecture.md` 的交易步驟、`engineering/specs/security-and-rollback.md` 的現有保證。
- Chefer AppCipe 容器 e2e 腳本，帶 `BETTER_OS_E2E_CONTAINER=1` 守衛。

## Acceptance criteria

- [x] 乾淨的 Ubuntu 22.04 與 24.04 容器可安裝 daemon 套件並解出所有動態連結。
- [x] 容器內完成套件安裝、移除、重新安裝與 purge，dpkg 狀態相符；daemon 在真實 system bus 上取得名稱、回報協定版本、拒絕未授權請求且不留下 journal。
- [x] 未安裝 daemon 時 GUI 與 CLI 都回報明確錯誤，不會假裝成功。
- [x] 文件不再宣稱「沒有任何出貨路徑會套用套件變更」。
- [x] 四組 release/architecture 的封裝矩陣全數通過（CI run 30688730458）。
- [x] `cargo fmt`、workspace check/test、clippy `-D warnings` 全數通過。

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo test --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- `packaging/build-deb.sh` 與 `packaging/verify-deb.sh` 四組矩陣
- `chefer run packaging/e2e/appcipe.yml`（絕不在 host 上執行）——已通過，exit 0

## Out of scope

- 套件簽章、公開 APT repository、release channels。
- `dpkg --configure -a` 修復動作。

## Notes

Carried over from the retired delivery status record.

Tickets 11 through 14 completed the real path. `manager-daemon` is a D-Bus
system service authorized by polkit that revalidates every plan from scratch
against the host, applies it through local APT, health-checks what it applied,
and rolls back what it can. `manager-platform` fetches artifacts into a cache
named by checksum and reads installed versions from dpkg. The GUI runs the
transaction off the UI thread and offers cancellation only while it can still
be honored. Real execution is the default for both front ends; demo mode is
explicit and visible.

Local `cargo fmt --all -- --check`, `cargo check --workspace --offline`,
`cargo test --workspace` (21 suites, no failures), and `cargo clippy --workspace
--all-targets -- -D warnings` all passed. `packaging/build-deb.sh` and
`packaging/verify-deb.sh` built and verified all three packages for
ubuntu-24.04 on amd64, including the daemon's unit, polkit policy, and bus
config. A `ZED_HEADLESS=1` GUI launch stayed alive for the full smoke window.
CLI smokes confirmed that real mode without a daemon reports
`daemon.unavailable` and writes no state, that `--execution mock` still walks
the old lifecycle, and that reconciliation detects a recorded component dpkg
has never heard of and blocks planning for it.

What has not run: `packaging/test-daemon-e2e.sh` inside a container, and the
four-way release/architecture packaging matrix. The daemon has never faced a
real polkit or a real dpkg. That is milestone M17.
