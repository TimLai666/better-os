# Better Patch: the Fixes page

Better Patch finds known Linux desktop faults on this machine, says what is
wrong in plain words, fixes them, checks each fix, and undoes it. It is the
Fixes page of the control center (ADR 0017); the page's layout and copy are
`design/control-center-spec.md` section 9, and the work is ticket 90.

## What an entry is

An entry is typed Rust in `crates/patch-core`, with tests, for a fault someone
reproduced and a fix someone verified on a real machine. Nothing is fetched or
supplied at run time, and nothing runs through a shell. Only entries 3 and 4
need root, through the daemon's typed methods below.

Every entry implements `patch_core::Entry`:

| Step | Method | What it does |
| --- | --- | --- |
| Detect | `detection_reads`, `detect` | Names what it must read, then decides from the answers: found (one finding per subject), not present, or cannot check with a reason. |
| Describe | `describe` | The exact change as data (`ChangeLine`), whether undo is needed, and the entry's limits. The page turns these into words. |
| Plan | `plan_reads`, `plan` | Reads again right before the change, then returns the changes (`Change`) and the record of them. |
| Apply | `engine::fix` | Makes the planned changes through the host and keeps the record. |
| Check | `check_reads`, `check` | Reads again after the change: passed, still present, or cannot check. |
| Undo | `undo_reads`, `plan_undo` | Plans the reverse of exactly what the record says, from a fresh reading. |

An entry never touches the machine. The host (`patch_core::Host`,
implemented by `patch_platform::LinuxHost`) answers each `Read` and makes each
`Change`. What the host may run is the closed `patch_core::Command` list: a
fixed absolute path (`/usr/bin/fc-match`, `/usr/bin/fc-cache`,
`/usr/bin/flatpak`, `/usr/bin/journalctl`) with fixed arguments, plus a
validated Flatpak app ID in one case. A command that changes the machine is never run as a read.
`patch-platform/src/command.rs` is the only place that starts a process; a
test over both crates' source keeps it that way.

### An entry that needs root

The trait does not change. The entry's `plan` returns
`Change::Privileged(Privileged::…)`, which names one typed `manager-daemon`
method (`ReloadUcsi`, `RebindController`, `SetChoice`, ADR 0046). The method
has its own polkit action and takes no path, device name, or free text, as
ADR 0017 requires for every root write. The host sends it over D-Bus. The
undo is the reverse change in `plan_undo`. A fix that changes the machine
for every account is kept by the machine (`Entry::kept_by_machine`): its
record is what the daemon keeps, read with detection, and nothing goes into
the person's `fixes.json`. Adding such an entry touches `Privileged`, the
host's `change`, the daemon, and its policy file. It never adds a command.

## States

Each subject (the machine, or one app) is in one state, decided by
`patch_core::state::state` from what detection saw, whether a fix is
recorded, and whether it was just undone:

| State | When | zh-TW / English |
| --- | --- | --- |
| Not present | It looked and the fault is not here | 這台電腦沒有這個問題。 / This PC doesn't have this problem. |
| Found | The fault is here, nothing recorded | 這台電腦有這個問題。 / Found on this PC. |
| Fixed and checked | A fix is recorded and the fault is gone | 已修正，檢查通過。 / Fixed and checked. |
| Applied but still present | A fix is recorded and the fault is still there | 已套用，但問題還在。 / Applied, but the problem is still there. |
| Undone | The fix was undone in this window, until Check again | 已復原。 / Undone. |
| Cannot check | A read failed. This outranks a record: a fix whose fault can't be checked is never called fixed | 無法檢查：{reason} / Can't check: {reason} |
| Does not apply | The fault is known only on other models (entries 3 and 4); nothing is read into a fault and nothing is offered | 不適用於這台電腦。 / Doesn't apply to this PC. |

A fault can be found and its fix not offered (entry 2 at a fractional scale,
or with screens at different scales). The card says why, in place of the
state, and has no button.

Found and applied-but-still-present need the person: they are the pane's
count and Home's "{n} known problems found". A found fault whose fix is not
offered is listed on the page and not counted, since there is nothing to
press.

The page's description promises only what is true: a fix that changes a
setting can be undone, and a setting the person changes afterwards is left
as they made it (Undo skips it unless they confirm Undo all):
會變更你設定的修正都可以復原；修正之後你自己改過的設定，會保留你改的樣子。 / A fix
that changes your settings can be undone; a setting you change yourself
afterwards is left as you made it. An undo can also fail (the override file
was turned into a link, which is never written through); the page then says
so and the fix stays recorded. The font cache fix changes no setting (it
repairs a cache) and its card says it needs no undo.

## The record

`$XDG_STATE_HOME/better-os/fixes.json` (schema version 1) keeps every
applied fix that can be undone. For an override key it holds the key, the
value the fix wrote, and the value that was there before. It also says
whether the fix created the file and which groups it added.

- Undo puts a key back only while it still holds what the fix wrote. A key
  already back at its earlier value is left alone.
- A key that holds anything else was changed after the fix. It is shown as
  `已由其他程式變更，確認後才會取代` / `Changed by another program; confirm it
  to replace`, the wording and rule Better Defaults uses. Undo leaves it and
  keeps it recorded. **Undo all** is the confirmation that replaces it too.
- Keys the fix never wrote are never in the record, so an undo cannot reach
  anything the person added to the same file.
- A record file that cannot be read, or that a newer Better Manager wrote, is
  reported and never overwritten. The page then offers no Fix or Undo.
- A fix that needs no undo (the font cache) keeps no record. Its result is
  shown until the next Check again.
- The override file is written only while it still holds what the plan read
  (otherwise "changed while it was being fixed"). It is written beside the
  old file and renamed over it, keeping the file's permissions, and it is
  refused when it is a link.
- Editing touches only the lines of the keys the fix owns. Everything else
  stays byte for byte: comments, spacing around `=`, `\r\n` line endings, a
  last line with no newline, repeated keys and repeated groups. Reading
  follows GLib: a repeated group is one group and the last occurrence of a
  key wins, so the fix writes the occurrence that takes effect.
- When the record or the overrides are not at the default places (a custom
  `XDG_STATE_HOME`, `XDG_DATA_HOME`, or `FLATPAK_USER_DIR`), the window
  also keeps `~/.local/state/better-os/fixes-location.json` saying where
  they are, so removing Better Manager can find them; it is removed again
  when everything is at the defaults.

## Entry 1: blank text from a damaged fontconfig cache

Reproduced on 2026-11-11 (ticket 76): every label in an Electron app was
blank, and every font query answered with one file and an unknown style.

- **Detect.** `fc-match -f '%{file}\t%{style[0]}\n'` for `sans-serif`,
  `serif`, `monospace`, `sans-serif:bold`, `serif:italic`, and
  `sans-serif:lang=zh-tw`. The fault is that every answer names the same file
  and no answer has a style (empty or `unknown`). One file with a known style
  is a machine with one font, not a damaged cache. A missing `fc-match` or a
  failed query means cannot check.
- **Fix.** `fc-cache -f`, run as the person, which rebuilds only their own
  cache.
- **Check.** The same queries no longer all give the same file with no style.
- **Undo.** Not needed. The card says 這項修正不需要復原。 / This fix doesn't
  need undoing.

## Entry 2: half-size pointer over an Electron app

Reproduced on 2026-11-11 (ticket 76) with GitHub Desktop, Electron 32.3.0,
as a Flatpak at 200%: the pointer was half size, and running the app
through XWayland fixed it with the picture still sharp.

How Electron chooses Wayland changed, and the entry follows each version:

- Up to 37, Electron follows `ELECTRON_OZONE_PLATFORM_HINT` (`auto` or
  `wayland` puts it on Wayland in a Wayland session).
- In 38 the variable was removed: Electron runs on Wayland when
  `XDG_SESSION_TYPE` is `wayland`. Source: Electron's breaking changes,
  <https://github.com/electron/electron/blob/main/docs/breaking-changes.md>,
  "Removed: `ELECTRON_OZONE_PLATFORM_HINT` environment variable" under
  "Breaking API Changes (38.0)", read 2026-11-21. The fix that made 38 read
  `XDG_SESSION_TYPE` is electron/electron#48301 (merged 2025-09-12).
- The newest releases first look at which display servers are reachable
  (`WAYLAND_SOCKET`, `WAYLAND_DISPLAY`, a `wayland-0` socket; `DISPLAY`),
  use the only one there is, and fall back to `XDG_SESSION_TYPE` when both
  or neither are: `docs/api/command-line-switches.md` and
  electron/electron#54523 (merged 2026-10-08), from Chromium 157 on main and
  backported by hand.

- **Detect.** All of these must hold (when only the Electron version is
  missing, see "cannot tell" below):
  - the session is Wayland (`XDG_SESSION_TYPE`);
  - an installed Flatpak app is Electron: it ships Electron's own `version`
    file beside its `resources.pak` (in its files or up to two folders
    down), or it is built on `org.electronjs.Electron2.BaseApp`;
  - with its metadata and every override layered as Flatpak layers them
    (system global, system app, user global, user app), it is granted the
    `wayland` socket, its `XDG_SESSION_TYPE` is not overridden to something
    else, and: with Electron 38 or later, nothing more; with an older
    Electron, or none known, its `ELECTRON_OZONE_PLATFORM_HINT` is `auto` or
    `wayland`;
  - some logical monitor's scale, from Mutter's
    `org.gnome.Mutter.DisplayConfig.GetCurrentState`, is above 1.

  An app on Flathub's Electron base whose `version` file is not found (it
  is looked for in the app's files and two folders down), with the Wayland
  socket and no hint, is the case where an Electron 38 or later would be on
  Wayland and an older one would not. It is listed as 無法判斷。 / Can't
  tell., with the reason and no Fix, never as "no problem".

  The scale is read only when there is a candidate app, and Mutter has five
  seconds to answer. Outside GNOME (the Better OS session on cosmic-comp is
  coming) nothing reports the scale yet, and the entry says at once
  無法檢查：這個桌面環境還不能提供螢幕縮放比例。 / Can't check: this desktop
  doesn't report its display scale yet. A missing session type, unreadable
  apps, or (with a candidate) an unreadable scale means cannot check.
- **Offered only where verified.** The fix is offered when every monitor
  has the same whole-number scale above 1 (200%, 300%). With a fractional
  scale (125%, 150%) GNOME 46 draws an XWayland window at 1x and stretches
  it, so the whole app turns blurry: the fault is listed with that reason and
  no Fix. With monitors at different whole-number scales (200% and 100%)
  XWayland draws the app at one scale, so it is blurry or the wrong size on
  the other; that case was not verified, and the fault is listed with that
  reason and no Fix either.
- **Fix.** Writes the person's own override file for that app only,
  `$XDG_DATA_HOME/flatpak/overrides/<app-id>`: the same keys `flatpak
  override --user --socket=x11 --nosocket=fallback-x11 --nosocket=wayland
  --env=ELECTRON_OZONE_PLATFORM_HINT=x11 --env=XDG_SESSION_TYPE=x11
  <app-id>` would write, each one doing a part:
  - without the `wayland` socket Flatpak unsets `WAYLAND_DISPLAY` and
    `WAYLAND_SOCKET` and binds no Wayland socket
    (`common/flatpak-run-wayland.c`, `flatpak_run_add_wayland_args`), so the
    newest Electron finds only X11;
  - `XDG_SESSION_TYPE=x11` decides it for Electron 38 and later;
  - `ELECTRON_OZONE_PLATFORM_HINT=x11` decides it up to 37;
  - `x11` granted with `fallback-x11` denied gives the app a `DISPLAY`
    (with `fallback-x11` granted, `--socket=x11` alone leaves it empty).

  Every other line of the file stays as it was. The change takes effect the
  next time the app starts, and the card says so.
- **Screen sharing.** Through XWayland, the app's screen sharing may show
  only its own windows and other X11 apps. WebRTC's
  `DesktopCapturer::IsRunningUnderWayland`
  (`modules/desktop_capture/desktop_capturer.cc`) is true only when
  `XDG_SESSION_TYPE` is `wayland` and `WAYLAND_DISPLAY` is set, and its X11
  screen capturer is used only when that is false
  (`modules/desktop_capture/screen_capturer_linux.cc`); Chromium's
  `CanUsePipeWire()` (`content/public/browser/desktop_capture.cc`) is true
  only in a Wayland session. All three were read on webrtc.googlesource.com
  and chromium.googlesource.com at `main` on 2026-11-21. The fix sets
  `XDG_SESSION_TYPE=x11` and removes `WAYLAND_DISPLAY`, so the app takes the
  X11 capturer, and an X11 capturer under XWayland sees only X clients. It
  has not been tried. The card's cause line says it before Fix is pressed
  (…但分享畫面時可能只看得到 X11 程式的視窗。 / …but its screen sharing may then
  show only X11 windows.), and the limits list says it was read from the
  source and not tried.
- **Check.** `flatpak override --user --show -- <app-id>`, Flatpak's own
  reading of the file, holds every key the fix wrote, and with the app's
  metadata, every other override layer, and its Electron version taken
  together as the app will start, it no longer runs on Wayland.
- **Undo.** Removes exactly those keys, or puts back the values they had. It
  removes the groups the fix added when they are left empty, and the file
  when the fix created it and it is left saying nothing. When every socket
  change is taken back, the `sockets` value goes back exactly as the person
  wrote it (`x11;ssh-auth` stays without a trailing `;`), from the text the
  record kept.

When the fix stops being needed: no Electron release note or issue found on
2026-11-21 says the pointer size on Wayland was fixed (searched
electron/electron issues and pull requests for the pointer and cursor on
Wayland), so the entry cannot tell when the override can go. It reads the
app's Electron version from its `version` file and shows it on the card
(使用 Electron {version}。 / Uses Electron {version}.), and the card says to
undo the fix by hand after the app updates to see whether its pointer is
right on its own.

Limits, which the page states:

- It was verified on a Flatpak only. An app installed any other way is
  offered nothing.
- An app is recognized by Electron's own version file or Flathub's
  Electron base app; one that ships Electron any other way is not.
- Offered only at one whole-number scale above 1, as above.
- Screen sharing from the app may show only X11 windows, as above.
- With no known Electron version and no hint, an app is not reported: an
  older Electron runs through XWayland on its own, and a newer one would
  have shipped its version file.

## Entries 3 and 4: the USB-C faults of two ASUS models

Ticket 101, ADR 0046. Seen on the owner's ASUS Vivobook S 14 M5406WA (BIOS
M5406WA.314, kernel 7.0.0-42 and -43, Secure Boot on); the UCSI half also on
the S 15 M5506WA (linux-usb, 2026-06-23). Secure Boot stays on and no key is
enrolled (owner, 2026-11-21): the recovery uses only what the running kernel
already has. Both entries apply only to a model in
`crates/hardware-core/data/usb-recovery-machines.txt`, matched by DMI
`board_name` (M5406WA, M5506WA). On any other machine each says
不適用於這台電腦。 / Doesn't apply to this PC. on "All known problems", is never
under "Problems found", and offers nothing; its kernel log is not even
classified.

### What is read

- **The machine** (`hardware_platform::usb`): `/sys/class/dmi/id`, the
  `/sys/class/typec/portN` entries, every `ucsi-source-psy-*` supply's
  `online`, the mains adapter's `online` (whether a charger is plugged in
  at all), and every xHCI function under `/sys/bus/pci/devices` (class
  0x0c0330): vendor, device, the bound driver, and the root hubs and devices
  under it from `/sys/bus/usb/devices`' links. No USB device's own
  attribute is read: on a host whose hub work hangs, reading one blocks for
  good (`usb1/product` did, 2026-11-14). The daemon's choices,
  `/var/lib/better-os/usb-recovery.json`, readable by everyone.
- **This boot's kernel log**, classified by
  `hardware_core::usb_recovery::classify`, never shown or sent as text:
  `journalctl --boot=0 --output=cat --grep=… _TRANSPORT=kernel + _UID=0
  SYSLOG_IDENTIFIER=better-manager-daemon` (the second match is the daemon's
  own recovery markers). It classifies:
  - the UCSI timeouts: `ucsi_acpi … (-110)` lines, `failed to re-enable
    notifications (-110)` counted apart (written about ten seconds after a
    resume);
  - an xHCI host's death: `xhci_hcd ADDRESS: HC died`, with the `IO_PAGE_FAULT`,
    `xHC restore state timeout`, and `PCI post-resume error` lines before it;
  - each suspend attempt (`PM: suspend entry` to `PM: suspend exit`), and
    whether it failed because `usb_hub_wq` would not freeze (a `Freezing …
    failed` line followed by `workqueue usb_hub_wq:`), and how many of the
    last attempts in a row did;
  - hung hub work: that failed freeze, or the worker a hung-task report
    names (`INFO: task … blocked for more than …`, its own lines, then
    `Workqueue: usb_hub_wq hub_event`, which ends it; any other line ends
    it first). The same line in any other stack dump (a warning during
    enumeration) is not hung work, grepped or not: the query keeps the
    first lines of every dump too;
  - the daemon's markers after a verified recovery (`usb-recovery: ucsi_acpi
    reloaded`, `usb-recovery: controller rebound ADDRESS`). A fault before
    the last marker is not failing now. The query is read as JSON with each
    entry's transport: a line counts as the kernel's only when the kernel
    wrote it (`_TRANSPORT=kernel`), and from the daemon's own stream only
    the two exact marker forms are taken, so nothing another process writes
    into that stream reads as a fault, a sleep, or a recovery. A reload is
    marked only when both ports are back and a plugged-in charger shows;
    one that did not is still failing and may be tried again.
- **Who reads the log.** On Ubuntu 24.04 and 22.04 the system journal is
  readable by `root`, `adm`, and `systemd-journal` (the journal files' group
  and access lists); `dmesg` is not used, since `kernel.dmesg_restrict=1`
  makes it need `CAP_SYSLOG`. The page runs `journalctl` itself when the
  person's account can open this machine's `system.journal` (persistent or
  volatile). Otherwise it asks the daemon's `org.betteros.UsbRecovery1.
  Findings`, an ungated read-only call that answers with the classified
  findings and never a log line. Neither: 無法檢查：讀不到這次開機的核心紀錄。 /
  Can't check: this boot's kernel log can't be read.

### Entry 3: the USB-C interface

- **Found** when a UCSI timeout came after the last verified reload (or
  there was none): 其中一個 USB-C 連接埠不再充電 / One USB-C port stops charging.
  Cause: 這款電腦的 USB-C 介面常在睡眠後停止回應，連接埠可能因此不再充電。重設這個介面，
  兩個連接埠就會恢復，直到下次睡眠。 / On this model the USB-C interface stops
  answering, often after sleep, and a port can stop charging. Resetting the
  interface brings both ports back until the next sleep. The card says how
  often: 這台電腦開機以來，USB-C 介面停止回應了 {n} 次，其中 {m} 次在睡眠後。 / Since
  this PC started, the USB-C interface stopped answering {n} times, {m} of
  them after sleep.
- **Fix.** Turns on the reload after each sleep (`SetChoice
  ucsi-automatic`), and reloads now when it is failing (`ReloadUcsi`). The
  exact change: 每次睡眠後，如果 USB-C 介面停止回應，就自動重設（重新載入 ucsi_acpi 驅動程式）。
  現在先重設一次。 / After each sleep, resets the USB-C interface when it
  stopped answering (reloads the ucsi_acpi driver). Resets it now.
- **Check.** No timeout after the last verified reload.
- **Undo.** `SetChoice ucsi-automatic off`: 復原會把這項功能關掉，其他都不會變。 /
  Undo turns this off again. Nothing else changes.

### Entry 4: the dead controller and the blocked sleep

- **Found** when an xHCI host died this boot and no verified rebind came
  after: 睡眠之後，一側的 USB 停止運作 / USB on one side stops working after
  sleep. Cause: 這款電腦醒來時，一側 USB-C 的 USB 控制器可能停止運作。卡住的時候電腦無法再進入
  睡眠，闔上螢幕時會整晚一直重試。 / On this model the USB controller of one USB-C
  side can stop working when the PC wakes. While it is stuck, the PC can't
  go to sleep again, and with the lid closed it keeps trying all night. The
  card names the controller (位址 {address} 的 USB 控制器在睡眠後停止運作。 / The USB
  controller at {address} stopped working after sleep.), the sleeps it
  blocked (睡眠因此失敗了 {n} 次，最近連續 {m} 次。 / Sleep failed {n} times because of
  it, the last {m} in a row.), and, when its hub work is stuck, that only a
  restart helps (它卡住了，只有重新開機才能恢復。請先存好檔案再重新開機。 / It's stuck,
  and only restarting the PC brings it back. Save your work, then restart.).
- **Fix.** Turns on the after-sleep rebind and the overnight guard
  (`SetChoice controller-automatic`), and rebinds the controller now
  (`RebindController`) unless its hub work is stuck; then it asks for no
  rebind at all, and the check says the problem is still there with the
  restart line.
- **Check.** No dead controller without a verified rebind after it.
- **Undo.** `SetChoice controller-automatic off`, which turns the power-off
  choice off too.

### Reset USB-C

On a found card of either entry, a second button, 重設 USB-C / Reset USB-C,
runs the same daemon action at any time, without turning anything on
(`ReloadUcsi` on entry 3, `RebindController` on entry 4; not offered while
the hub work is stuck). The card then says what came back, until Check
again:

| Result | zh-TW / English |
| --- | --- |
| Both ports, charger online | 已重設：兩個 USB-C 連接埠都恢復了，充電器也在充電。 / Reset: both USB-C ports are back, and the charger is charging. |
| Both ports, no charger plugged in | 已重設：兩個 USB-C 連接埠都恢復了。目前沒有接上充電器。 / Reset: both USB-C ports are back. No charger is plugged in. |
| Both ports, charger unreadable | 已重設：兩個 USB-C 連接埠都恢復了。讀不到是否接著充電器。 / Reset: both USB-C ports are back. Whether a charger is plugged in can't be read. |
| Fewer ports | 已重設，但 {m} 個 USB-C 連接埠只恢復了 {n} 個。 / Reset, but only {n} of {m} USB-C ports came back. |
| Charger plugged in, not shown | 已重設，但接著的充電器還是沒有出現在 USB-C 連接埠上。 / Reset, but the plugged-in charger still doesn't show on a USB-C port. |
| Controller back | 已重新啟動：USB 控制器恢復了，上面接著 {n} 個裝置。 / Restarted: the USB controller is back, with {n} devices on it. |
| Controller not back | 已重新啟動，但 USB 控制器沒有恢復。請重新開機。 / Restarted, but the USB controller didn't come back. Restart the PC. |
| Inside the cooldown (added) | 這是剛才那次重設的結果，不會連續重設兩次。 / This is the reset from a moment ago; it isn't run twice in a row. |

A refusal is a sentence in the error bar, never a key: 這個核心無法載入 USB-C 驅動程式的檔案，所以沒有重設連接埠，也沒有變更任何東西。 / This kernel can't load the USB-C driver's file, so the ports weren't reset. Nothing was changed. (the kernel cannot decompress the distribution's file: no `/sys/module/compression` naming its format and no build config saying so); 不需要重設：USB-C 連接埠運作
正常。 / Nothing to reset: the USB-C ports are working.; USB 控制器卡住了，只有重新開機
才能恢復。 / The USB controller is stuck, and only restarting the PC brings it
back.; 停止運作的控制器不是 Better OS 可以重新啟動的那種。 / The controller that stopped
isn't one Better OS may restart.; 沒有完成驗證，所以沒有變更任何東西。 / Authentication
wasn't completed, so nothing changed.; 另一個重設正在進行，請稍後再試。 / Another
reset is running. Try again in a moment.; 沒有安裝 Better OS 的特權服務，所以無法執行。
/ The Better OS privileged service isn't installed, so this can't be done.

### After each sleep

The daemon package ships `/usr/lib/systemd/system-sleep/better-os-usb-recovery`
(systemd 255 reads hooks only from there), one `exec` of the daemon's
program with systemd-sleep's two arguments. With neither fix on it reads the
choice file and returns. Otherwise it queues one transient unit ordered
after the sleep service, so nothing holds the resume, which:

- with entry 3 on, watches up to 25 s for the timeout written after a resume
  and reloads `ucsi_acpi`;
- with entry 4 on, rebinds a dead controller whose hub work is free, and
  runs the overnight guard below.

Both use the daemon's own helpers and their limits (ADR 0046).

### The overnight loop: what is safe

On 2026-11-13 the dead controller's hub work kept every suspend from
working; logind retried about every 40 s for 11 hours with the lid closed
(1,672 attempts), each one dropping the Wi-Fi. With entry 4 on:

- After the second failed attempt in a row for that reason, the person is
  told once per run of failures, as a desktop notification in their
  language: 這台電腦現在無法進入睡眠 / This PC can't go to sleep right now, and
  睡眠之後，一側的 USB 停止運作，電腦也因此一直無法進入睡眠。重新開機就能修好。 / USB on one
  side stopped working after sleep, and that keeps the PC from sleeping.
  Restarting the PC fixes it. A sleep that works ends the run. Traditional
  Chinese goes to a person whose language (AccountsService, else the
  system's) is `zh_TW`, `zh_HK`, or `zh_Hant`, the rule every Better OS
  notification follows (`better_core::locale`). A notification gets ten
  seconds; a session bus that does not answer does not hold the run.
- **Default: notify only.** The card, once the fix is on, says 闔上螢幕後睡眠一直
  失敗時，Better OS 只會通知你。 / When sleep keeps failing with the lid closed,
  Better OS only notifies you., with 改成關機 / Turn off the PC instead.
- **Power-off is the person's choice**, made on that card (an administrator
  authenticates): 闔上螢幕、沒有接擴充座或外接螢幕時，睡眠一直失敗，Better OS 會關機。開著的應用程式裡沒存的內容會不見。
  / When sleep keeps failing with the lid closed and no dock or external screen, Better OS turns off the PC. Work you haven't saved in open apps is lost., with 只通知我 / Only
  notify me. The card says what it never does: 螢幕打開時、接著擴充座或外接螢幕時、有程式暫緩關機時（例如正在更新）、Better Manager 正在變更套件時、還有其他登入的工作階段時（包括你自己在別的螢幕或從網路登入的），或有帳號設定成登出後服務繼續執行時，都不會關機。 / It never turns off the PC while the lid is open, while it's docked or an external screen is connected, while something holds off shutting down (an update, for example), while Better Manager is changing packages, while any other session is signed in (yours on another screen or over the network included), or while an account is set to keep its services running after signing out.
- Why these: with the lid open the person may be at the machine, and the
  loop is not burning a battery in a bag. A closed lid alone cannot tell a
  bag from a desk: a laptop on a dock or with an external screen may be in
  use with its lid shut, so logind's `Docked` must say no (not knowing is
  docked). A `block` inhibitor on `shutdown` is someone saying a power-off
  now would break their work (unattended-upgrades, a disc burn), and
  root's `PowerOff` would not be stopped by it, so the guard asks logind's
  `ListInhibitors` itself and treats not knowing as held. Better
  Manager's own privileged service marks a transaction it runs
  (`/run/better-os/daemon-busy`), and a power-off then could leave dpkg
  half way; whether apt itself holds a logind inhibitor while dpkg runs is
  not known, so the marker is checked whatever it does. Any other login
  session holds work nobody chose to lose: another person's, or the same
  person's over SSH or on a tty (a long job), of any of systemd's login
  classes (`user`, `user-early`, `user-incomplete`, `user-light`,
  `user-early-light`; an unknown class counts too), and so does an account
  set to linger, whose services run with no session at all. All of this
  is read again right before the power-off, after the notifications. The lid state is logind's
  `LidClosed`; not knowing is not "closed". `PowerOff(false)` goes through
  logind, so its delay inhibitors (an application saving) still get their
  time. At most one power-off is asked for per run of failures.

### Limits, which the page states

- Offered only on the two models it was seen on.
- Entry 3 resets the interface after each sleep; the firmware fault stays
  until ASUS or the kernel fixes it (the upstream report is
  `engineering/upstream/101-ucsi-and-xhci-on-asus-m5406wa.txt`).
- When the controller's hub work is stuck only a restart helps.
- Neither has run on a real occurrence yet: ticket 101 waits for the next
  release on the owner's machine.

### The fix is kept by the machine

These two fixes change the machine for every account, so their record is
the daemon's choice file, not the person's `fixes.json`
(`Entry::kept_by_machine`, `Entry::machine_records`): every account sees the
same state, and Undo turns the choice off. Removing Better Manager or the
daemon package forgets the choices (`--release-hardware`, and the daemon's
postrm), so the hook, which goes with the daemon package, does nothing.

## Where the page checks

- A real window checks the person's machine.
- A demo window (`BETTER_MANAGER_EXECUTION=mock`) checks a fixture machine
  seeded with every fault (the two USB-C faults on an M5406WA included, with
  a fixture daemon that changes only the fixture's files) under `$XDG_RUNTIME_DIR/better-os/fixes-demo`, so
  a demo never changes this machine. The runtime directory must be a real
  directory the person owns with no access for others, and the two folders
  below it are made 0700 or must be theirs and closed to others' writes
  (`patch_platform::fixture::private_demo_root`). There is no shared fallback
  such as `/tmp`, where another account could lay links in advance for a Fix
  to follow: without a private runtime directory the demo checks nothing and
  says 示範模式找不到只屬於你的資料夾來放範例電腦，所以這裡不做任何檢查。 / Demo
  mode has no private folder for its example PC, so nothing is checked here.
- A debug build takes `BETTER_OS_DEBUG_FIXES_ROOT`, a fixture machine for
  sandbox screenshots (`patch_platform::fixture`).
- A test build checks `/nonexistent/better-os-test-root`, where nothing can
  be read.

The page checks when the window opens (for Home and the count), each time the
page is shown, and on Check again, always off the UI thread. A Fix or an Undo
runs off the UI thread too, and the page checks again afterwards. An update
restart waits while either is running.

A window is often still open while its package is removed. The prerm's
stamp, `/run/better-os/better-manager-removal-run`, is what says a removal
is running: dpkg's selection alone does not, since it stays `deinstall`
after an abandoned removal while the package is `installed`
(`better_core::self_update::removal`). `/run/better-os` exists from boot
(the package's `tmpfiles.d` entry), and the update watcher watches it as
well as the program file, so the window learns of the removal from the
stamp while its own file is still there, and reports
`UpdateState::Removed` (also once the file is gone and dpkg no longer has
the package). The page then changes nothing and says
Better Manager 正在被移除，這一頁現在不會變更任何東西。 / Better Manager is
being removed, so this page changes nothing now. A Fix or an Undo also
looks for the stamp right before it writes, and refuses while it is there,
so nothing is written that the removal has already undone. A removal dpkg
left part way (it asks for `deinstall` and the package is not `installed`,
with no stamp: a power cut) is said, with what finishes it: 移除 Better
Manager 中途停住了。執行「sudo apt install --reinstall better-manager」可以裝回來，
或再移除一次完成移除。 / Removing Better Manager stopped part way. Run "sudo apt
install --reinstall better-manager" to put it back, or remove it again to
finish.

## Removing Better Manager

A fix Better Manager applied is something Better OS added to the machine, so
it does not outlive Better Manager (AGENTS.md). Better Manager cannot remove
itself, so apt is how it goes. Its package ships the before-removal program
`/usr/lib/better-os/before-removal/better-manager`
(`patch_platform::removal`), and its maintainer scripts run it once per
person (`packaging/maintainer/before-removal-accounts.sh`, with
`accounts.sh` and `account-step.sh`):

- **The library.** The account functions (`accounts.sh`,
  `account-step.sh`, the fragment, after their header) are kept in a shell
  variable of the maintainer script, never in a file: nothing in `/tmp` or
  `$TMPDIR` can be swapped for root to run, and a missing or full temporary
  directory fails nothing. It is built only for `prerm remove`, `postinst
  abort-remove`, and `postrm remove|purge`, and neither building it nor
  running it can fail the script. `accounts.sh` documents the interface for
  any other script that runs a small step as each account.
- **Who.** Every account in the login range (`UID_MIN`..`UID_MAX` from
  `/etc/login.defs`, 1000..60000 by default) that `getent passwd` lists, so
  LDAP, sssd, and systemd-homed accounts are reached, whose name is a plain
  name (`[A-Za-z0-9._-]`, not starting with `-`; any other is skipped and
  counted, and nothing of it reaches a program text), and whose home lies,
  by the mount table alone, on a mounted local file system, or on a network
  one while the person has a session (`/run/user/<uid>`). Root never `stat`s
  a home to decide this, so no automount is triggered.
- **Time limits.** Each person's whole step, root's own checks of the home
  included, runs in its own shell under `timeout -s KILL 30`, and the
  listing under the same limit. The whole walk stops after 120 seconds and
  says how many people it did not reach. A home on a dead server costs at
  most one step and a line naming the account; the removal goes on and
  dpkg's lock is let go.
- **How.** As the person (`setpriv`), in a session of its own (`setsid`),
  with standard input from `/dev/null`, its output passed through a loop
  that prefixes each line with the account's name (no `sed` program built
  from data), and only `HOME`, a fixed `PATH`, `LC_ALL=C`, and the run's
  stamp in its environment, so it holds nothing of the terminal apt runs in.
- **prerm remove** first deletes any older stamp (this line comes before
  every other fragment of the prerm, so a later failure of another one can
  never leave an older stamp for abort-remove to read), then makes up a
  stamp for this dpkg run, keeps it in
  `/run/better-os/better-manager-removal-run`, and runs the program. The
  program finds the record and the overrides at the defaults or where the
  location file says, following each path it names only when the deepest
  part of it that exists is the person's own: inside their home, or owned
  by them (a custom `XDG_DATA_HOME` such as `/data/tim/share` is
  followed). It sets the record and each override file a fix
  wrote aside in `~/.local/state/better-os/fixes-removal-undo.json`, stamped.
  Then it undoes every recorded fix that has an undo, through the page's own
  undo path and rule: a key the person changed after the fix is left as they
  made it and named. A fix whose undo fails (the file became a link, changed
  meanwhile, cannot be written) stays recorded and its file is named; the
  rest of the record is deleted. Anything left or named makes it exit 3,
  which is a warning, never a reason to stop the removal.
- **postinst abort-remove** runs it with `--undo` and the stamp: only a
  set-aside file with this run's stamp is put back (every override file and
  the record as they were before the prerm). One with another stamp, or with
  none, is stale and deleted unused.
- **postrm remove or purge** deletes, as the person, the set-aside file,
  the location file, and what an interrupted write of the record, the
  set-aside file, or the location file can leave (`.fixes.json.tmp`,
  `fixes-removal-undo.json.tmp`, `fixes-location.json.tmp`), and the stamp.
  A record that still holds a fix whose undo failed is kept.

A removal that fails after the prerm (dpkg does not call abort-remove then)
leaves Better Manager installed with its fixes undone and the record gone.
At its next start, when no removal is running (no stamp) and dpkg does not
report one stopped part way, the window finds the set-aside file, deletes
it, and says on the Fixes page how many fixes that removal undid. While a
removal runs, the set-aside file is that removal's and is never touched:
an abort-remove's `--undo` needs it (移除 Better Manager 沒有完成，但已經復原了 {n} 項修正。… / Removing Better
Manager didn't finish, and it had already undone {n} fixes. …); each fault
that came back is found again by the next check and can be fixed again.
Nothing is reapplied from a file that might be older than this installation.

A fix with no undo (the rebuilt font cache) keeps no record and is left: it
restored the machine rather than adding to it. The program needs no session,
so it never exits 4.

What removal still does not reach:

- An encrypted home that is not mounted (an fscrypt or ecryptfs home of a
  person who is not logged in, a systemd-homed home that is not active): its
  files cannot be read, so its fixes stay until the person's next login,
  when there is no Better Manager left to undo them. The override is then
  the person's to remove (`flatpak override --user --reset <app-id>` removes
  all of that app's overrides).
- `getent passwd` lists only what the directory service enumerates; sssd
  with enumeration off lists local accounts only.
- Only the user and the system Flatpak installations are read; those listed
  in `/etc/flatpak/installations.d` are not.

The removal has been run only by tests and by a container check in CI
(`packaging/test-daemon-e2e.sh`: an account with uid 1000, a record and an
override, `apt-get remove better-manager` under a time limit); that check's
first run is CI's.

## Candidate faults, not built

Recorded in ticket 90 and not an entry, because the fault is not confirmed
yet:

- The panel taking long to light up again on battery after the screen blanks
  (amdgpu PSR). It would be a daemon entry, as above, that sets and restores
  a kernel parameter, with its power cost stated. It waits for the owner's
  trial of `amdgpu.dcdebugmask=0x10`.

## Done when

- Each entry's detection (present and absent), fix, check, and undo are
  tested against a fixture machine in a scratch directory
  (`crates/patch-platform/tests/entries.rs`); entries 3 and 4 also on a
  machine that is not affected, and their decisions against kernel log
  excerpts from ticket 101 (`crates/hardware-core/tests/usb_recovery.rs`).
- The daemon's USB-C methods, helpers, and after-sleep run are tested
  against a fake machine and a private bus
  (`crates/manager-daemon/tests/usb_recovery.rs`), and the helpers' units
  in CI's container check (`packaging/hardware-unit/check.sh`).
- A test over `patch-core` and `patch-platform` asserts that nothing names a
  shell, that `patch-core` does no I/O, and that only `command.rs` starts a
  process, and only what a `Command` names
  (`crates/patch-platform/tests/no_shell.rs`).
- The page is screenshotted in both appearances and both languages, at 1600
  and 960, with a fault present and with none.
