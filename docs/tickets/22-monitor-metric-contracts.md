# 22 — Better Monitor 有真實的量測契約與 Linux collectors

**Epic:** Better Monitor
**User Story:** 使用者看到的每一個數字都來自真實的 `/proc` 或 `/sys`，而且「沒有這個數字」的五種原因彼此分得清楚，不會被當成 0。
**Blocked by:** 03-monitor-contracts
**Status:** done

## Why

`monitor-core` 原本只有一個 mock `Sample`，三個 `f32` 欄位加上一個 `Option`。
它沒辦法表達 issue #16 要求的東西：unit、semantic type、source、support state、
sampling behavior，以及 unknown / unsupported / permission-denied / stale /
真實的 0 這五種互相獨立的狀態。只要 `Option<f32>` 是唯一的表達方式，
「這台機器沒有 PSI」和「PSI 是 0」就會長得一模一樣，Overview 會把沒被觀測的
系統畫成健康的系統。

## What it delivers

### `monitor-core`：型別化的量測與能力契約

- `MetricId`（封閉字元集、長度上限）、`Unit`、`SemanticType`、`MetricSource`、
  `SamplingBehavior`（`Instant` / `CounterDelta` / `KernelAveraged`，含
  minimum interval 與 freshness budget）、`MetricDescriptor`。
- `Observation` 五態：`Value`、`Stale`、`Unknown`、`Unsupported`、
  `PermissionDenied`，各自帶原因。`Value(Unsigned(0))` 是真實測到的 0。
- `SupportState` 與 `MetricCapability`：把靜態 catalog 與這一輪實際證明到的
  支援狀態配對起來。
- `Collector` trait 不回傳 `Err`：讀不到是資料，不是錯誤。整個子系統不可用時
  用 `CollectorHealth` 表達。
- `Timestamp` 同時帶 wall clock 與 monotonic clock，rate 一律除 monotonic，
  系統時間被校正不會變成假的尖峰。
- `MonitorStore` 保留 incident 與 export redaction 邊界，並新增 per-metric
  `coverage()`，讓觀測缺口在匯出裡是明確資料而不是空白。

### 新 crate `monitor-collectors-linux`

六個 collector，全部直接讀 `/proc` 與 `/sys`，不執行任何指令、不解析工具輸出。
每個讀取都經過 `Roots`，所以測試跑的是與正式環境完全相同的程式路徑。

- `linux.cpu`：`/proc/stat` 全機與每顆邏輯 CPU 的十種時間分類、`/proc/loadavg`、
  context switch / interrupt / fork 速率、cpufreq 目前與上下限頻率、governor、
  hwmon 封裝與每核心溫度。
- `linux.memory`：`/proc/meminfo` 三十個欄位、swap、`/proc/vmstat` 的 page in/out、
  swap in/out、major / minor fault、reclaim 與 OOM。
- `linux.pressure`：`/proc/pressure/{cpu,memory,io}`，沒有 `CONFIG_PSI` 時整個
  子系統一次回報 unsupported。
- `linux.process`：name、state、ppid、uid 與使用者名稱、CPU time 與 delta、RSS、
  swap、virtual、threads、fd 數、start time、runtime、nice、priority、cgroup
  路徑，cmdline 在 privacy flag 之後。
- `linux.storage`：`/proc/diskstats` delta，只留真實整顆 block device。
- `linux.network`：`/sys/class/net` 加 `/proc/net/dev` delta、link speed、型別與狀態。

### Fixtures 與 overhead

- 從真實主機錄下 `snapshot-a` / `snapshot-b` 兩份 `/proc` 與 `/sys` 快照，
  另有手寫的 `synthetic-a` / `synthetic-b`（可算出精確期望值）、`truncated`、
  `malformed`、`no-psi`，共 597 個 fixture 檔。
- `examples/overhead.rs` 與 `tests/overhead.rs` 實測整組 collector 取樣 N 次的
  wall 與 CPU 成本，數字記在 `docs/monitor-collector-sources.md`。

### Source traceability

`docs/monitor-collector-sources.md` 逐一記錄每個 collector 的 upstream spec 與
版本、adoption mode、與來源的語意差異、以及證明該解讀的測試名稱，另含 `sysinfo`
的評估結論與實測 overhead 數字。

## Acceptance criteria

- [x] mock `Sample` 被型別化的 metric / capability 契約取代。
- [x] metric 定義包含 unit、semantic type、source、support state 與 sampling behavior。
- [x] unknown、unsupported、permission-denied、stale 與真實的 0 是五個不同狀態，且有測試證明彼此不相等。
- [x] 新 crate `monitor-collectors-linux` 直接讀 `/proc` 與 `/sys`，沒有任何 CLI 解析。
- [x] 每個 parser 都接受 root path 參數，測試餵的是錄下來的 fixture tree。
- [x] 每個 parser 都有 fixture 測試，含 truncated 與 malformed 輸入。
- [x] delta-based metric 有兩次取樣的測試，且第一次取樣回報 unknown 而不是 0。
- [x] 每個 collector 都有 source traceability 紀錄。
- [x] 有實測的 overhead 數字，不是估計值。
- [x] `monitor-gui` 改用新契約後仍可編譯，且行為不變。

## Verification

以 crate 為範圍執行，避免在 worktree 重建 GPUI：

- `cargo fmt --all -- --check` 通過。
- `cargo check -p monitor-core -p monitor-collectors-linux` 通過。
- `cargo test -p monitor-core -p monitor-collectors-linux` 通過：monitor-core 26 個測試，
  monitor-collectors-linux 117 個 unit test 加 14 個 integration test。
- `cargo clippy -p monitor-core -p monitor-collectors-linux --all-targets -- -D warnings` 通過。
- `cargo check -p monitor-gui` 通過（monitor-gui 有改動）。
- `cargo run -p monitor-collectors-linux --release --example overhead -- 100` 實際執行並取得數字。

完整 workspace gate 在合併後於主 checkout 執行。

## What this caught

- `/proc/net/dev` 會把介面名稱填滿固定欄寬。錄到的 `tailscale0:` 後面沒有空白，
  用 whitespace 切會把第一個計數值併進名稱。改成先切冒號。
- `/proc/stat` 的 `user` 已經含 guest，`nice` 已經含 guest_nice。直接把十個欄位
  相加會重複計算，在跑虛擬機的機器上壓低所有比例。
- `/proc/vmstat` 的 `pgpgin` / `pgpgout` 儘管有 `pg` 前綴，計的是 KiB 不是 page，
  而同一個檔案裡的 `pswpin` / `pswpout` 計的才是 page。
- 錄下來的測試機是 AMD，`k10temp` 只發布 `Tctl`，沒有 `Core N` label。
  這正好是「每核心溫度必須是 unsupported 而不是 0」的真實案例。
- PID 會被重用。不比對 start time 就直接算 CPU time delta，會把上一個 process
  的累計值算到新 process 頭上。

## Out of scope

- 真實的 Overview、Apps、Processes 畫面。ticket 23 負責。
- GPU、電池與能源、SMART、cgroup 層級的 app grouping。
- 持久化的時序儲存、retention 與 downsampling，那需要先有 ADR 與 benchmark。
- 每個 process 的網路與 GPU 歸屬，issue #16 明確延後。
- 多 socket 機器的 per-core 溫度封裝解析，目前沒有硬體可驗證。
- 10,000 個 process 的大規模 benchmark 場景。
