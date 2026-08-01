# Better Monitor ↔ GNOME Resources v1.10.2 parity checklist

Baseline:

- GNOME Resources tag: `v1.10.2`
- GNOME Resources commit: `b99edbf`
- baseline date: 2026-03-06
- Better Monitor implementation: Rust + GPUI + `longbridge/gpui-component`

This checklist is the merge gate for the GNOME Resources parity portion of Better Monitor. Better OS additions such as Overview, History, Incidents, Diagnostics, analysis, and export are tracked separately and do not replace Resources behavior.

## Status legend

| Status | Meaning |
|---|---|
| ✅ Implemented | Present in the branch and backed by real state or an explicit support state |
| 🟨 Partial | Main surface exists, but required behavior, persistence, accessibility, or platform coverage is incomplete |
| ⬜ Missing | Not implemented yet |
| 🧩 Hardware dependent | Implemented adapter or state model exists, but real hardware validation is still required |
| 🧪 Validation required | Code exists, but GNOME Wayland, scaling, theme, keyboard, or accessibility review is not complete |

## Shared UI component inventory

| Component | Status | Current implementation / missing work |
|---|---:|---|
| Adaptive split shell | ✅ | The desktop sidebar remains above the breakpoint; below 980 logical px it is replaced by a horizontally scrollable page rail and reduced content padding. |
| Resource sidebar row | 🟨 | Label, detail, selection, progress bar, and mini-graph modes exist. Resource icons and accessible names are incomplete. |
| Selected-page header | 🟨 | Page label and description exist. Device-specific model/identifier and narrow back button are incomplete. |
| Metric card | ✅ | Reusable metric card exists in `monitor-gui`. |
| Live chart panel | 🟨 | Reusable area-chart card exists. Text alternatives, highest-value annotation, non-color encoding, and irregular-time handling are incomplete. |
| Device page shell | 🟨 | Graph-first and grouped property layout exists on several pages, but is not a shared typed component yet. |
| Property group / row | ✅ | Reusable section and property-row helpers exist. |
| Search toolbar | ✅ | Shared Apps / Processes search toolbar exists. Processes supports `|` terms. |
| Virtualized data table | ✅ | GPUI component `DataTable` is used for both Apps and Processes. |
| Multi-selection toolbar | ✅ | Processes separates row focus from an explicit multi-PID selection set and batch toolbar. |
| Split action button | ⬜ | Required for End Application and alternate force/pause/resume actions. |
| Context menu | 🟨 | Processes has a row context menu. Apps and batch-selection context menus remain incomplete. |
| Confirmation dialog | ✅ | Graceful and force-stop actions use confirmation dialogs for Apps and Processes. |
| Information dialog | ✅ | Application Information and Process Information dialogs exist; Process Options remains a separate GPUI window. |
| Permission state | 🟨 | Typed permission-denied panels are used for process actions. A narrow Polkit helper flow is still missing. |
| Stale-process state | ✅ | Selections are pruned and typed stale feedback is rendered separately when a PID has already disappeared. |
| Empty / unsupported / error state | 🟨 | A shared typed panel now covers unavailable, permission-denied, stale, collector-error, success, and info states. Collector-specific adoption and unknown-state copy remain incomplete. |
| Toast / result banner | ✅ | Action feedback uses the shared typed support-state panel instead of inferring semantics from message text. |
| Settings row / switch | ✅ | Reusable preference rows and switches exist. |
| Runtime language selector | 🟨 | Shared `system`, `en-US`, and `zh-TW` locale state is persisted and switches shell/navigation copy immediately. Full page catalogs and pseudo-long mode remain incomplete. |
| Accessible chart summary | ✅ | Every shared chart card presents current, average, minimum, maximum, and sample-count text alongside the visual graph. |
| Keyboard focus model | 🟨 | Native controls are focusable. Complete table cell navigation, predictable focus order, shortcuts, and dialog focus trapping are missing. |
| Authenticated hardware-information prompt | ⬜ | Required for DMI memory details when sysfs is insufficient. |

The answer to “are all required UI components built?” is therefore **no**. The core visual primitives exist, but parity-specific interaction, accessibility, adaptive, localization, and privileged-boundary components are still being implemented.

## Application shell and navigation

| Requirement | Status | Notes |
|---|---:|---|
| Persistent resource sidebar | ✅ | Apps, Processes, CPU, Memory, dynamic GPU/NPU/drive/network/battery entries, and Better Monitor pages exist. |
| Live sidebar meter | ✅ | Progress bar and mini-graph modes exist. |
| User-facing device detail | 🟨 | Model/interface detail exists where exposed. Icons and some stable identifiers remain incomplete. |
| Selected device title/model | 🟨 | Page title exists; device-specific secondary title is incomplete. |
| Adaptive narrow layout | ✅ | Uses the 980 logical px breakpoint, replaces the sidebar with a compact page rail, and reacts directly to the current viewport size. |
| Restore last selected page | 🟨 | The selected page is persisted and restored. Per-device selection and window geometry remain missing. |
| Restore window size/maximized state | ⬜ | Not persisted. |
| Pause graphical updates when hidden | ⬜ | Manual graph pause exists; visibility-driven throttling is missing. |
| Collection independent from rendering | 🟨 | Manual graph pause preserves collection. Long-running service ownership remains outside this PR. |

## Apps

| Requirement | Status | Notes |
|---|---:|---|
| cgroup/systemd/application grouping | 🟨 | Typed grouping with explicit fallback exists; grouping quality and identity coverage need real-session tests. |
| App icon and name | 🟨 | Name exists; icon resolution is missing. |
| Search | ✅ | Search filters app groups. |
| Sortable table | ✅ | Apps uses a virtualized GPUI `DataTable`; supported metric columns sort. |
| Remember sort column/direction | ✅ | Apps persists its last sort column and direction in the monitor table-sort preferences. |
| Column visibility | ✅ | Required preference switches exist. |
| Memory / CPU | ✅ | Aggregated. |
| Drive read/write speed and totals | ✅ | Aggregated. |
| GPU / GPU memory / encoder / decoder | 🟨 | Columns exist with explicit unavailable values; attribution adapters are missing. |
| Swap / combined memory | ✅ | Aggregated. |
| Application information dialog | ✅ | Implemented in the Apps action column. |
| End Application split action | ⬜ | Separate row buttons exist; Resources-style split action is missing. |
| Graceful end / force / pause / resume | ✅ | Signals are implemented with explicit denied/unavailable results. |
| Context menu | ⬜ | Missing. |
| Confirmation dialogs | ✅ | End and Force actions require confirmation. |
| Background/system grouping | 🟨 | Grouping reason is visible; dedicated visual separation needs refinement. |
| Temporary refresh hold during interaction | ⬜ | Missing. |

## Processes

| Requirement | Status | Notes |
|---|---:|---|
| Searchable virtualized table | ✅ | GPUI `DataTable`; `|` multi-term behavior implemented. |
| Sortable columns | ✅ | Supported columns sort; unavailable GPU columns and Options do not claim sorting. |
| Remember sort column/direction | ✅ | Processes restores and persists its last sort column and direction. |
| Multi-selection | ✅ | Dedicated switches maintain a PID set independently from row focus. |
| Column visibility | ✅ | Required settings exist. |
| Name / PID / user / memory / CPU | ✅ | Implemented. |
| Read/write speed and totals | ✅ | Implemented. |
| GPU / GPU memory / encoder / decoder | 🟨 | Visible explicit unavailable state; attribution missing. |
| Total/user/system CPU time | ✅ | Implemented from Linux process counters. |
| Priority / swap / combined memory / command line | ✅ | Implemented with privacy work still required for command arguments. |
| Process information dialog | ✅ | Dedicated information dialog is available from the Processes toolbar. |
| Process options dialog | 🟨 | Separate GPUI window exists with priority and affinity controls; final CI and real-session interaction review pending. |
| Priority/niceness control | 🟨 | Linux `setpriority` boundary exists. Polkit path for privileged changes is missing. |
| CPU affinity control | 🟨 | Linux affinity read/write and per-CPU switches exist. Real process tests pending. |
| Graceful end / force / pause / resume | ✅ | Single-process actions exist. |
| Batch actions | ✅ | End, Force, Pause, and Resume operate on the selected PID set with confirmations for destructive actions. |
| Single/multi-selection context menus | ⬜ | Missing. |
| Confirmation/error/permission/stale states | 🟨 | Errors are reported; dedicated confirmation, permission, and stale components are missing. |
| Keyboard cell navigation and semantics | 🧪 | GPUI table baseline exists; full keyboard/accessibility audit pending. |
| Temporary refresh hold during interaction | ⬜ | Missing. |

## Processor

| Requirement | Status | Notes |
|---|---:|---|
| Total usage graph | ✅ | Implemented. |
| Total/logical toggle | ✅ | Preference exists. |
| Logical CPU tiles/graphs | ✅ | Implemented. |
| Per-logical CPU frequency/utilization | 🟨 | Utilization exists; frequency coverage requires hardware validation. |
| Temperature graph / highest value | 🟨 | Current temperature/property coverage exists; history/highest presentation needs completion. |
| Maximum speed | 🟨 | Exposed where available. |
| Logical/physical/socket counts | 🟨 | Topology collector exists; hardware validation pending. |
| Uptime / virtualization / architecture / model | ✅ | Implemented where exposed. |
| Normalized/aggregate CPU setting | ✅ | Preference exists. |

## Memory

| Requirement | Status | Notes |
|---|---:|---|
| Memory used/total/percentage graph | ✅ | Implemented. |
| Swap used/total/percentage graph | 🟨 | Implemented, but unavailable/no-swap presentation needs explicit visual review. |
| Slots used/total | ⬜ | Needs SMBIOS/DMI adapter and authenticated helper fallback. |
| Speed / form factor / type / type detail | ⬜ | Needs SMBIOS/DMI adapter and typed support states. |
| Narrow authenticated helper | ⬜ | Missing. GPUI must stay unprivileged. |

## GPU

| Requirement | Status | Notes |
|---|---:|---|
| One page per GPU | 🧩 | Dynamic pages exist. |
| Total utilization | 🧩 | DRM/sysfs adapter exposes values where available. |
| Encoder/decoder | 🧩 | Explicit partial support. |
| Video memory | 🧩 | Used/total or used-only semantics supported. |
| Temperature / highest | 🟨 | Current value exists where exposed; highest-history presentation incomplete. |
| Power / active cap / max cap | 🧩 | Partial driver-dependent support. |
| GPU and memory clocks | 🧩 | Partial driver-dependent support. |
| Manufacturer/model/PCI/driver/link | 🧩 | Metadata collector exists; hardware validation required. |
| App/process attribution | ⬜ | Missing typed vendor/driver adapters. |

## NPU

| Requirement | Status | Notes |
|---|---:|---|
| One page per NPU | 🧩 | Dynamic pages exist for detected accelerator devices. |
| Usage/memory/temperature/power/clocks | 🧩 | Partial driver-dependent support. |
| Manufacturer/model/PCI/driver/link | 🧩 | Metadata support exists; hardware validation required. |

## Drives

| Requirement | Status | Notes |
|---|---:|---|
| One page per drive | ✅ | Dynamic pages exist. |
| Activity | ✅ | `/proc/diskstats`-based activity. |
| Read/write speed and totals | ✅ | Implemented. |
| Highest read/write speed | ⬜ | Current history model does not persist per-device maxima. |
| Type/path/capacity/writable/removable | 🟨 | Metadata exists; support needs real-device validation. |
| Link type/speed | 🧩 | Shown where sysfs exposes it. |
| Friendly identity | 🟨 | Model fallback exists. |
| Virtual-drive setting | ✅ | Implemented and persisted. |

## Network

| Requirement | Status | Notes |
|---|---:|---|
| One page per interface | ✅ | Dynamic pages exist. |
| Receive/send throughput and totals | ✅ | Implemented. |
| Highest receive/send throughput | ⬜ | Per-interface maxima are not retained. |
| Manufacturer/driver/interface/address | 🟨 | Metadata exists; privacy presentation requires review. |
| Wi-Fi SSID | ⬜ | NetworkManager typed adapter missing. |
| Link details/speed | 🧩 | Shown where sysfs exposes it. |
| Bytes/bits setting | ✅ | Implemented and persisted. |
| Virtual-interface setting | ✅ | Implemented and persisted. |

## Batteries

| Requirement | Status | Notes |
|---|---:|---|
| One page per battery | ✅ | Dynamic pages exist. |
| Charge percentage/state | ✅ | Implemented. |
| Charge graph | ⬜ | Current battery page uses cards rather than retained battery history. |
| Power use graph / highest power | ⬜ | Current value exists where available; history/max missing. |
| Health/design capacity/cycles/technology | 🧩 | Power-supply metadata adapter exists. |
| Manufacturer/model/device | 🧩 | Implemented where exposed. |

## Preferences

| Requirement | Status | Notes |
|---|---:|---|
| Decimal/binary units | ✅ | Persisted. |
| Celsius/Kelvin/Fahrenheit | ✅ | Persisted. |
| 3 s / 2 s / 1 s / 500 ms / 250 ms | ✅ | Persisted. |
| Sidebar bars/mini graphs | ✅ | Persisted. |
| Graph point count | ✅ | Persisted and bounded. |
| Graph grids | ✅ | Persisted. |
| Sidebar details/descriptions | ✅ | Persisted. |
| Virtual drives/interfaces | ✅ | Persisted. |
| Network bytes/bits | ✅ | Persisted. |
| Apps/Processes columns | ✅ | Persisted. |
| Apps/Processes sorting | ✅ | Both tables persist sort column and direction. |
| Logical CPU display | ✅ | Persisted. |
| CPU normalization | ✅ | Persisted. |
| Detailed priority | ✅ | Persisted. |
| Last page/window/maximized | ⬜ | Missing. |

## Localization, accessibility, and validation

| Requirement | Status | Notes |
|---|---:|---|
| `en-US` | 🟨 | Shell, navigation, selected-page headings, and language settings are locale-driven; remaining page copy is still being cataloged. |
| `zh-TW` | 🟨 | Shell, navigation, selected-page headings, and language settings have Traditional Chinese copy; remaining page copy is incomplete. |
| System language | ✅ | Shared Better OS locale resolves `LANG` to `zh-TW` or `en-US`. |
| Runtime switching | 🟨 | Monitor switches and persists locale without restart; search placeholders, table headers, dialogs, and full page content still need catalog coverage. |
| Pseudo-long locale tests | ⬜ | Missing. |
| Light/dark themes | 🧪 | Uses theme tokens; real-session review pending. |
| 100/125/150% scaling | 🧪 | Not validated. |
| Keyboard-only operation | 🟨 | Basic controls work; full flows and shortcuts incomplete. |
| Accessible chart summaries | ✅ | Shared chart cards expose localized current, average, minimum, maximum, and sample-count summaries. |
| Accessible table alternatives | 🟨 | Table semantics depend on GPUI component; audit and explicit labels missing. |
| No color-only meaning | 🟨 | Text values accompany most colors; chart-series encoding needs review. |
| Unknown/unsupported/stale/denied/zero distinct | 🟨 | Support-state direction exists; dedicated state components and action-level stale handling incomplete. |
| Real GNOME Wayland review | ⬜ | Required before ready-for-review. |

## Next implementation order

1. Keep Process Options compiling and validated.
2. Build the remaining shared parity interaction components: context menu and split action.
3. Implement process multi-selection and batch actions.
4. ✅ Convert Apps to a virtualized sortable table with persisted sort state and information/actions dialogs.
5. Add persisted shell state and temporary refresh holds.
6. Add localization runtime and pseudo-long tests.
7. Add SMBIOS/DMI memory adapter with a narrow authenticated helper boundary.
8. Add per-device history/maxima and NetworkManager Wi-Fi metadata.
9. Add accessibility summaries and keyboard/focus validation.
10. Run real GNOME Wayland parity review at all required themes, locales, and scale factors.
