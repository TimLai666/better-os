# 14 — daemon 封裝、接線與文件重談

**Epic:** Better Manager 真實系統整合
**User Story:** 使用者安裝 Better Manager 後，系統上就有一個可被審查的特權服務，
而且專案文件描述的保證與程式實際行為一致。
**Blocked by:** 13-gui-real-execution
**Status:** todo

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
- 文件同步：AGENTS.md 的禁令與待決清單、ENG.md 測試矩陣、README 的能力描述、
  `docs/architecture.md` 的交易步驟、`docs/security-and-rollback.md` 的現有保證。
- Chefer AppCipe 容器 e2e 腳本，帶 `BETTER_OS_E2E_CONTAINER=1` 守衛。

## Acceptance criteria

- [ ] 乾淨的 Ubuntu 22.04 與 24.04 容器可安裝 daemon 套件並解出所有動態連結。
- [ ] 容器內完成 install → update → rollback，dpkg 狀態與 journal 相符。
- [ ] 未安裝 daemon 時 GUI 與 CLI 都回報明確錯誤，不會假裝成功。
- [ ] 文件不再宣稱「沒有任何出貨路徑會套用套件變更」。
- [ ] 四組 release/architecture 的封裝矩陣全數通過。
- [ ] `cargo fmt`、workspace check/test、clippy `-D warnings` 全數通過。

## Verification

- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo test --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- `packaging/build-deb.sh` 與 `packaging/verify-deb.sh` 四組矩陣
- Chefer AppCipe 容器 e2e（絕不在 host 上執行）

## Out of scope

- 套件簽章、公開 APT repository、release channels。
- `dpkg --configure -a` 修復動作。
