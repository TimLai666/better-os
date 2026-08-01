# 16 — 授權成功路徑的真實驗證

**Epic:** Better Manager 真實系統整合
**User Story:** 使用者按下安裝、通過授權之後，元件真的會被裝上去；失敗時也真的會
被還原回去。
**Blocked by:** 15-container-e2e
**Status:** done（回滾的真實驗證未含在內，見驗收條件）

## Why

到票 15 為止，所有驗證證明的都是「未授權會被拒絕」。沒有任何一次執行證明過
「授權之後真的會裝起來」。連帶地，`AptGetDriver`、健康檢查與回滾這三段一直只對
`FakeAptDriver` 跑過——也就是說，使用者最主要的動線從來沒有被實際走完一次。

polkit 的密碼框是 polkit 的責任，不是這個專案要驗證的東西。這張票要驗證的是：
**當 polkit 回答 authorized 時，daemon 會正確完成整筆交易。** 因此容器裡用一條
polkit rule 讓授權通過，換到的是第一次真正執行到 apt、真正的健康檢查、真正的
回滾。

## What it delivers

- 容器內的 polkit rule，讓測試身分在 `org.betteros.manager.apply-transaction`
  上取得授權，藉此走到成功路徑。
- 端對端斷言：`StageArtifact` 後 `ApplyTransaction` 成功、dpkg 真的多了那個套件、
  回傳的 outcome 健康狀態為 healthy、交易 journal 標記 completed、
  回滾記錄內容與實際情況相符。
- 移除路徑同樣走一次，確認 dpkg 真的少了那個套件。
- 漂移拒絕：宣稱一個 dpkg 不同意的前一版本時，服務拒絕且不改變任何東西。

## Acceptance criteria

- [x] 授權通過後 `ApplyTransaction` 回傳成功，且 dpkg 確實安裝了該套件。
- [x] 回傳的 `StepReport` 健康狀態為 healthy，`applied_version` 與 dpkg 一致。
- [x] 交易 journal 為 completed，且可用 `GetStatus` 讀回。
- [x] 移除交易執行後 dpkg 確實不再有該套件。
- [ ] 安裝後失敗的情境會真的回滾，且回報的復原狀態與 dpkg 實際狀態相符。
      （尚未做：需要一個能讓真實健康檢查失敗的元件，目前 catalog 裡沒有這種東西。）
- [x] 未授權仍然被拒絕（票 15 的斷言不得因為這張票而失效）。

## Verification

- `chefer run packaging/e2e/appcipe.yml`
- CI 的四組 release/architecture 矩陣

## What this caught

真的把授權路徑跑起來之後，立刻抓到兩個只有這條路徑會暴露的問題：

- `DbusPrivilegedExecutor::execute_plan` 會死鎖。它另開一條執行緒讀 `StepProgress`
  signal，但那個 iterator 不會因為 `ApplyTransaction` 回來就結束，於是
  `std::thread::scope` 永遠等不到它。這是出貨程式的 bug，CLI 與 GUI 只要真的連上
  daemon 就會卡住。已改為只從 outcome 取結果——下載進度本來就由 client 端回報，
  daemon 端的套用相對短。
- 被拒絕的計畫是以「呼叫成功、outcome 為 failed」的形式回來的（這是對的，client
  兩種情況都需要 reports），所以判斷成敗必須看 outcome 而不是看呼叫。

## Out of scope

- polkit 認證代理本身的互動行為。
- 桌面 session 上的手動驗證。
