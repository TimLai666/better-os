# 15 — 容器內的真實端對端驗證

**Epic:** Better Manager 真實系統整合
**User Story:** 維護者可以在一個用完即丟的容器裡，看到特權服務面對真實的 dpkg、
真實的 system bus 與真實的 polkitd 時確實照設計行為，而不必拿自己的機器冒險。
**Blocked by:** 14-daemon-packaging-and-cutover
**Status:** done

## What it delivers

- `packaging/e2e/` 的 Chefer AppCipe 與 Dockerfile，用 `chefer run
  packaging/e2e/appcipe.yml` 在一次性的 Ubuntu 24.04 容器裡跑完整檢查。
- `packaging/test-daemon-e2e.sh` 擴充到真的把服務跑起來：啟動 system bus 與
  polkitd，執行 daemon 執行檔，確認它取得 bus name、在未授權情況下仍可回報協定
  版本、拒絕未授權的 `ApplyTransaction`，且拒絕後不留下任何 journal。
- CI 在 amd64 的封裝工作中一併跑這個容器檢查，讓封裝迴歸由建置抓到而不是由使用者
  抓到。

## Acceptance criteria

- [x] 守衛有效：沒有 `BETTER_OS_E2E_CONTAINER=1` 或不是 root 時直接拒絕執行。
- [x] daemon 套件在一個完全沒有圖形函式庫的映像檔中安裝成功。
- [x] unit、busconfig、polkit 政策與狀態目錄都落在正確位置。
- [x] `better-monitor` 可經 apt 安裝、移除、再安裝，dpkg 狀態每次都相符。
- [x] daemon 在真實 system bus 上取得 `org.betteros.Manager1`。
- [x] `ProtocolVersion` 不需授權即可讀取，且回報 1。
- [x] 未授權的 `ApplyTransaction` 被拒絕，且沒有寫入任何交易 journal。
- [x] purge 後 `/var/lib/better-os` 與 `/var/cache/better-os` 都不存在。

## Verification

- `chefer run packaging/e2e/appcipe.yml`（ubuntu-24.04 / amd64）——exit 0
- CI run 30688730458：四組 release/architecture 全數通過，含原生 arm64
- `packaging/test-daemon-e2e.sh` 在 host 上直接執行會被守衛拒絕（exit 2）

## Out of scope

- 帶認證代理的完整授權路徑（見下）。
- 容器裡沒有可互動的 polkit agent，所以這裡驗證的是「未授權必定被拒絕」，
  而不是「授權後會成功」。後者目前只對 fake authorizer 測過。
