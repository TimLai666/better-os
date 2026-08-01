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
| Adaptive split shell | 🟨 | Persistent desktop sidebar exists. Narrow-window sidebar replacement, back navigation, and breakpoint behavior are missing. |
| Resource sidebar row | 🟨 | Label, detail, selection, progress bar, and mini-graph modes exist. Resource icons and accessible names are incomplete. |
| Selected-page header | 🟨 | Page label and description exist. Device-specific model/identifier and narrow back button are incomplete. |
| Metric card | ✅ | Reusable metric card exists in `monitor-gui`. |
| Live chart panel | 🟨 | Reusable area-chart card exists. Text alternatives, highest-value annotation, non-color encoding, and irregular-time handling are incomplete. |
| Device page shell | 🟨 | Graph-first and grouped property layout exists on several pages, but is not a shared typed component yet. |
| Property group / row | ✅ | Reusable section and property-row helpers exist. |
| Search toolbar | ✅ | Shared Apps / Processes search toolbar exists. Processes supports `|` terms. |
| Virtualized data table | ✅ | GPUI component `DataTable` is used for Processes. Apps still uses custom rows rather than a virtualized sortable table. |
| Multi-selection toolbar | ⬜ | Required for batch process actions. |
| Split action button | ⬜ | Required for End Application and alternate force/pause/resume actions. |
| Context menu | ⬜ | Required for Apps and single/multi-process selections. |
| Confirmation dialog | ⬜ | Required before destructive app/process actions. |
| Information dialog | 🟨 | Process Options has a separate GPUI window. Application Information and Process Information dialogs are missing. |
| Permission state | 🟨 | Process control returns Linux errors. Dedicated permission UI and narrow Polkit helper flow are missing. |
| Stale-process state | ⬜ | Process disappearance clears selection, but action-specific stale handling is missing. |
| Empty / unsupported / error state | 🟨 | Unsupported device page helper exists. Dedicated unknown, stale, permission-denied, and collector-error visuals are incomplete. |
| Toast / result banner | ✅ | Action result banner exists. |
| Settings row / switch | ✅ | Reusable preference rows and switches exist. |
| Runtime language selector | ⬜ | `en-US`, `zh-TW`, system language, and pseudo-long test mode are missing. |
| Accessible chart summary | ⬜ | Text summaries and table alternatives are missing. |
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
| Adaptive narrow layout | ⬜ | Needs breakpoint, sidebar replacement, and back behavior. |
| Restore last selected page | ⬜ | Settings model does not yet persist page/device selection. |
| Restore window size/maximized state | ⬜ | Not persisted. |
| Pause graphical updates when hidden | ⬜ | Manual graph pause exists; visibility-driven throttling is missing. |
| Collection independent from rendering | 🟨 | Manual graph pause preserves collection. Long-running service ownership remains outside this PR. |

## Apps

| Requirement | Status | Notes |
|---|---:|---|
| cgroup/systemd/application grouping | 🟨 | Typed grouping with explicit fallback exists; grouping quality and identity coverage need real-session tests. |
| App icon and name | 🟨 | Name exists; icon resolution is missing. |
| Search | ✅ | Search filters app groups. |
| Sortable table | ⬜ | Current Apps surface is custom rows and not sortable. |
| Remember sort column/direction | ⬜ | Missing. |
| Column visibility | ✅ | Required preference switches exist. |
| Memory / CPU | ✅ | Aggregated. |
| Drive read/write speed and totals | ✅ | Aggregated. |
| GPU / GPU memory / encoder / decoder | 🟨 | Columns exist with explicit unavailable values; attribution adapters are missing. |
| Swap / combined memory | ✅ | Aggregated. |
| Application information dialog | ⬜ | Missing. |
| End Application split action | ⬜ | Separate row buttons exist; Resources-style split action is missing. |
| Graceful end / force / pause / resume | ✅ | Signals are implemented with explicit denied/unavailable results. |
| Context menu | ⬜ | Missing. |
| Confirmation dialogs | ⬜ | Missing. |
| Background/system grouping | 🟨 | Grouping reason is visible; dedicated visual separation needs refinement. |
| Temporary refresh hold during interaction | ⬜ | Missing. |

## Processes

| Requirement | Status | Notes |
|---|---:|---|
| Searchable virtualized table | ✅ | GPUI `DataTable`; `|` multi-term behavior implemented. |
| Sortable columns | ✅ | Supported columns sort; unavailable GPU columns and Options do not claim sorting. |
| Remember sort column/direction | ⬜ | Current sort is in-memory only. |
| Multi-selection | ⬜ | Table currently records one selected PID. |
| Column visibility | ✅ | Required settings exist. |
| Name / PID / user / memory / CPU | ✅ | Implemented. |
| Read/write speed and totals | ✅ | Implemented. |
| GPU / GPU memory / encoder / decoder | 🟨 | Visible explicit unavailable state; attribution missing. |
| Total/user/system CPU time | ✅ | Implemented from Linux process counters. |
| Priority / swap / combined memory / command line | ✅ | Implemented with privacy work still required for command arguments. |
| Process information dialog | ⬜ | Inline details exist; dedicated dialog is missing. |
| Process options dialog | 🟨 | Separate GPUI window exists with priority and affinity controls; final CI and real-session interaction review pending. |
| Priority/niceness control | 🟨 | Linux `setpriority` boundary exists. Polkit path for privileged changes is missing. |
| CPU affinity control | 🟨 | Linux affinity read/write and per-CPU switches exist. Real process tests pending. |
| Graceful end / force / pause / resume | ✅ | Single-process actions exist. |
| Batch actions | ⬜ | Depends on multi-selection toolbar and confirmations. |
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
| Apps/Processes sorting | ⬜ | Missing persistence. |
| Logical CPU display | ✅ | Persisted. |
| CPU normalization | ✅ | Persisted. |
| Detailed priority | ✅ | Persisted. |
| Last page/window/maximized | ⬜ | Missing. |

## Localization, accessibility, and validation

| Requirement | Status | Notes |
|---|---:|---|
| `en-US` | 🟨 | English strings exist but are not in a locale catalog. |
| `zh-TW` | ⬜ | Missing. |
| System language | ⬜ | Missing. |
| Runtime switching | ⬜ | Missing. |
| Pseudo-long locale tests | ⬜ | Missing. |
| Light/dark themes | 🧪 | Uses theme tokens; real-session review pending. |
| 100/125/150% scaling | 🧪 | Not validated. |
| Keyboard-only operation | 🟨 | Basic controls work; full flows and shortcuts incomplete. |
| Accessible chart summaries | ⬜ | Missing. |
| Accessible table alternatives | 🟨 | Table semantics depend on GPUI component; audit and explicit labels missing. |
| No color-only meaning | 🟨 | Text values accompany most colors; chart-series encoding needs review. |
| Unknown/unsupported/stale/denied/zero distinct | 🟨 | Support-state direction exists; dedicated state components and action-level stale handling incomplete. |
| Real GNOME Wayland review | ⬜ | Required before ready-for-review. |

## Next implementation order

1. Keep Process Options compiling and validated.
2. Build shared parity interaction components: confirmation dialog, context menu, split action, multi-selection toolbar, support-state panel, and adaptive navigation.
3. Implement process multi-selection and batch actions.
4. Convert Apps to a virtualized sortable table with persisted sort state and information/actions dialogs.
5. Add persisted shell state and temporary refresh holds.
6. Add localization runtime and pseudo-long tests.
7. Add SMBIOS/DMI memory adapter with a narrow authenticated helper boundary.
8. Add per-device history/maxima and NetworkManager Wi-Fi metadata.
9. Add accessibility summaries and keyboard/focus validation.
10. Run real GNOME Wayland parity review at all required themes, locales, and scale factors.
