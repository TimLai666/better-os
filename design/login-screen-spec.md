# The login screen and the lock screen

A Better OS login screen laid out like the lock and login screen of Luma
(<https://simplyluma.com>, the Project Luma operating system), written in Rust,
and one lock screen inside the Better OS session that looks the same. This
file says what Luma's screens are, measured from its source; what Better OS
follows and what it does not take; every state of Better OS's screen with the
zh-TW and English copy; what the lock screen adds; how the two programs are
built (greetd, the compositor before anyone is logged in, GPUI, the session
lock protocol, the display manager switch on Ubuntu 24.04 and Zorin OS 18);
and the tickets that build it. It answers ticket 259.

The security review the greeter, the lock program, the login supervisor and
the GNOME unlock service need before any of them runs on a person's machine
is its own file,
[`engineering/specs/login-screen-security.md`](/engineering/specs/login-screen-security.md).
Who owns the login and lock path, and in what order Better OS takes it over
from GDM, is
[ADR 0044](/engineering/decisions/0044-better-os-login-screen-and-gdm.md)
(accepted on 2026-11-21, amended after review on 2026-11-22). The work is cut into tickets 260 to 269, 278 to 281 (270 to 277
were taken on another branch). The look follows
`DESIGN.md`; every control is a `better_ui::fluent` control and every icon
comes from `better_ui::icon_set`. The mockups in `design/login-screen/` draw
the lock screen's quiet view (`quiet`), the login screen's entry view
(`entry`), choosing among several accounts (`accounts`), the session choice
open (`session`), a wrong password with the wait (`wrong-password`), Caps Lock
on (`caps-lock`), the lock screen with notifications (`lock-notifications`)
and without them (`lock-private`), and a 960-wide screen (`narrow-960`), each
as `-light.html` and `-dark.html`, in zh-TW, in the technique of
`design/top-bar/`; and the text login the boot falls back to
(`text-fallback.html`, one file: a text console has no light or dark). The
English copy is in the tables below. The rendered images are in
`/home/tim/Developer/better-os-evidence/259/`.

Two requirements from the owner (2026-11-21) govern the whole design:

- **Locking never depends on GDM.** Every session Better OS offers can lock,
  whatever display manager started it (section 5.6).
- **The login screen always appears.** The failure of ticket 139, a boot
  that stays on the splash with no login screen, must not be possible on
  the Better OS path, whatever its cause (section 5.10).

Where this file and a ticket differ, the ticket decides behaviour and this
file decides layout and copy.

## 0. Decisions that are the owner's

**Decided by the owner on 2026-11-21: L1 = (B), L2 = (a), L8 = (a), each as
recommended; and on 2026-11-22: L3 = (b), L4 = (a) with the setting, L5 = (a),
L6 = (a), L7 = (a), each as recommended.** Every decision in this table is
answered.

A review on 2026-11-22 corrected the design without changing these answers:
L1's row says "a login prompt within 60 seconds"; that figure counted
stopping a unit as free, and with every stop counted it is **80 seconds, or
90 when the supervisor itself cannot run** (section 5.10). The other
corrections are listed in ADR 0044's amendment of 2026-11-22.

| # | Decision | Recommendation | What each choice costs |
| --- | --- | --- | --- |
| L1 | **Who owns the login and lock path** (ADR 0044; sections 5.6, 5.10, 9). (A) GDM keeps doing login; Better OS's lock screen only inside the Better OS session. (B) Better OS owns it on greetd: the Better OS greeter, a login supervisor that guarantees a login prompt within 60 seconds, and a GNOME unlock service so GNOME locks without GDM; GDM stays installed (Zorin's `zorin-os-desktop` depends on `gdm3`) and does not run; delivered in section 9's order, offered first as a choice in Better Manager. (C) As B, with a display manager Better OS writes itself instead of greetd. | **(B)**, in section 9's order. Whether it later becomes the default for everyone is a separate decision the owner takes after section 9's last step. | **A:** fails both requirements: GNOME's lock stays GDM's, and a GDM that starts and shows nothing still leaves the splash up (its unit quits Plymouth only when it *fails*, `gdm.service` `OnFailure=plymouth-quit.service`); ADR 0033's watch only treats it. **B:** three Better OS programs run as root (greetd is Ubuntu's; the supervisor and the unlock service are Better OS's) and the login path becomes Better OS's to keep working; greetd allows one session at a time, so no second person can log in while one is logged in; the machine is proven in VMs stage by stage before it reaches anyone. **C:** Better OS also writes the VT, PAM and session-start code greetd already has (about 2,800 lines of Rust in greetd 0.9.0), more root code of its own for no gain the supervisor does not give; it would allow several sessions and VT-switch locking, which L2 (b) shows is not safe anyway. |
| L2 | **How a GNOME session locks without GDM** (section 5.6). (a) A Better OS system service answers the exact D-Bus surface GNOME Shell asks GDM for (`org.gnome.DisplayManager`'s `Version` and `OpenReauthenticationChannel`, then a private `UserVerifier` connection running PAM); GNOME Shell is not patched. (b) Lock by leaving the session: a lock surface on another virtual terminal, as LightDM's `dm-tool lock` does. (c) Keep GDM running only to serve GNOME's lock. (d) Stop offering GNOME at the Better OS login screen. | **(a).** | **(a):** a second root service that runs PAM for a person who is already logged in, reachable only by that person's own session (security review); it follows an interface GDM owns and GNOME may change, so every GNOME update is checked against it (ticket 280's test). **(b):** fails the first invariant: switching back to the session's virtual terminal shows its content unless the session itself also hides it, which is GNOME's lock again; it also needs C. **(c):** not possible: GDM's daemon always manages `seat0` and shows its own greeter, and it is a dependence on GDM in the owner's sense. **(d):** no GNOME at the Better OS login screen: the cost of doing nothing else, not a solution. |
| L3 | **The quiet view on a wide screen** (section 3.4). (a) As Luma's desktop: the entry card is always on screen; the quiet view exists only on narrow screens. (b) The lock screen opens in the quiet view when the screen wakes and goes back to it after 60 seconds without input; the login screen always opens in the entry view. | **(b).** | **(a):** one view less to build; the password field is always one keystroke away, and so is the person's name and picture to anyone walking past. **(b):** a locked laptop shows only the time and the wallpaper until someone touches it, as Windows' and Luma's narrow lock screens do; the first key typed still goes into the field, so nobody loses a character. |
| L4 | **Notifications on the lock screen** (section 4.2). (a) Shown as Luma shows them: "While you were away", the application, the title, one line of the body and the time, at most three, with a setting on the Notifications page (ticket 238) to hide them. (b) Not shown at all. | **(a), with the setting.** | **(a):** what the owner asked for (Luma's lock screen); anyone at the keyboard can read previews, until the person turns the setting off; the lock program reads the notification service's history over the session bus. **(b):** nothing leaks; a person checks messages only after unlocking. |
| L5 | **The login screen's background** (section 3.2). (a) Better OS's own artwork, the same for every person. (b) The person's own wallpaper, copied by `manager-daemon` into a system place whenever they change it. | **(a)**; the lock screen uses the person's own wallpaper either way. | **(a):** the greeter reads no file a person chose; the login and lock screens differ in their picture only. **(b):** the two screens match; a privileged copy of a person-chosen image on every change, and the greeter decodes it before anyone is logged in, so a crafted image reaches the most exposed program on the machine. |
| L6 | **How GPUI gets the session lock protocol and a password field the input method leaves alone** (section 5.4). Neither GPUI at the workspace's revision nor GPUI Fast (ADR 0045, accepted) has them. (a) Write both in GPUI Fast's Linux backend, offer them upstream as pull requests, and carry them until merged in the owner's patched `gpui-fast-linux` (ADR 0045's Decision 3 mechanism), with a new record, because ADR 0043's rule, which ADR 0045 keeps for that patch, lets it carry only fixes. (b) Wait for upstream; until then the session keeps `cosmic-greeter` as its locker. (c) Draw the lock screen outside GPUI. | **(a).** | **(a):** the lock screen ships in GPUI like every other surface; two features in the patched crate until GPUI Fast takes them, each with the same review and Corresponding Source duty ADR 0043 gives a fix. **(b):** no fork change; the lock and login screens do not match for an unknown time. **(c):** a second toolkit for one program, outside `better-ui`, which AGENTS.md's single kit rule forbids. |
| L7 | **The compositor the greeter runs in** (section 5.3). (a) `cosmic-comp`, already installed with the session. (b) Cage, from Ubuntu's archive. | **(a).** | **(a):** no new package; layer-shell surfaces per output exactly as ticket 200 proved them; but the greeter depends on the session's closure, so it can be offered only where the session is installed. **(b):** one more package (`cage 0.1.5+20240127-2build1` with `xwayland` and wlroots 0.17); Cage 0.1.5 offers only `xdg_shell`, no layer shell, and stretches one window across every output or uses only the last one (section 5.3). |
| L8 | **What the boot falls back to when the Better OS login screen cannot appear** (section 5.10). (a) A text login on the first virtual terminal that says which stage failed, after one retry of the greeter and one in safe mode; GDM is never started on its own. (b) GDM is tried before the text login. | **(a).** | **(a):** a person whose graphics never come up types their user name and password at a text prompt that tells them what happened and how to go back to GDM; nothing depends on GDM. **(b):** a graphical login more often, when the fault is in Better OS's greeter and not in the graphics; but GDM is then part of the path the owner asked not to rely on, and a GDM that hangs (ticket 139) sits behind it again, so the supervisor must time GDM out too.  |

## Sources this design was checked against

Read on 2026-11-21. No web request carried a personal detail.

On the owner's machine, read only:

- Zorin OS 18.1 on Ubuntu 24.04, `gdm3 46.2-1ubuntu1~24.04.9+zorin1`
  (`dpkg-query -W`); `/etc/X11/default-display-manager` is `/usr/sbin/gdm3`
  and `/etc/systemd/system/display-manager.service` links to
  `/lib/systemd/system/gdm3.service`, which links to `gdm.service`
  (`ls -l`). `gdm.service` has `Conflicts=getty@tty1.service`.
- `/usr/lib/systemd/system/gdm.service`: `Conflicts=plymouth-quit.service`,
  "GDM takes responsibility for stopping plymouth, so if it fails for any
  reason, make sure plymouth still stops" with `OnFailure=plymouth-quit.service`,
  `Restart=always`, `BusName=org.gnome.DisplayManager`. So a GDM that is
  *active* and shows nothing never quits the splash.
  `plymouth-quit.service` runs `plymouth quit` with `TimeoutSec=20`;
  `plymouth-quit-wait.service` runs `plymouth --wait` with `TimeoutSec=0`.
  `plymouth 24.004.60-1ubuntu7.2`, `systemd 255.4-1ubuntu8.18`,
  `util-linux 2.39.3-9ubuntu6.6`.
- `apt-cache depends gnome-shell`: `Depends: gir1.2-gdm-1.0`, `Recommends:
  gdm3`; `zorin-os-desktop` has `gdm3` in `Depends` (`apt-cache show`), so
  GDM cannot be removed from Zorin OS without removing its desktop
  metapackage. `Gdm-1.0.typelib` is in `gir1.2-gdm-1.0` and `libgdm.so.1` in
  `libgdm1`, client libraries separate from the `gdm3` daemon.
- `/usr/share/dbus-1/system.d/gdm.conf` (from `gdm3`): only root may own
  `org.gnome.DisplayManager`; every user may call its
  `org.gnome.DisplayManager.Manager` interface and read its properties.
- `/etc/pam.d/common-session` line 30: `session optional pam_systemd.so`, so
  a text login works even when logind does not answer.
- `man agetty` ("ISSUE FILES"): `/etc/issue.d/*.issue` is shown after
  `/etc/issue`, and `--issue-file` takes a list of files (since 2.35).
- `apt-cache policy`: `greetd 0.9.0-4ubuntu1` and `cage
  0.1.5+20240127-2build1` in `noble/universe`, neither installed;
  `squeekboard` has no candidate; `orca 46.1-1ubuntu1`,
  `at-spi2-core 2.52.0-1build1`. `apt-cache show greetd`: `Provides:
  x-display-manager`, `Depends: libc6, libgcc-s1, libpam0g, adduser`,
  `Suggests: wlgreet`. `apt-cache show cage`: `Depends: …
  libwlroots12t64 (>= 0.17.0), … xwayland`.
- `/var/lib/dpkg/info/gdm3.config` and `gdm3.postinst` (lines 26 to 58): the
  debconf question `shared/default-x-display-manager` decides
  `/etc/X11/default-display-manager`, and `gdm3`'s postinst links
  `display-manager.service` to `/lib/systemd/system/<basename of that
  file>.service` on every configure, so every `gdm3` upgrade relinks it from
  the file.
- `/etc/pam.d/login`: `pam_faildelay.so delay=3000000` first, then
  `@include common-auth` (`pam_unix.so nullok`, `pam_sss.so`); no
  `pam_faillock` line. `/etc/pam.d/` has GDM's `gdm-password`,
  `gdm-fingerprint` and `gdm-smartcard*` services.
- `/var/lib/AccountsService/users/` is `drwx------ root root`: a greeter
  account cannot read it and must ask AccountsService over the system bus.
- `/etc/default/keyboard`: `XKBLAYOUT="us"`. `localectl`: `LANG=zh_TW.UTF-8`,
  `LC_TIME=lzh_TW`.
- `/usr/share/wayland-sessions/`: `sway.desktop`, `zorin.desktop`,
  `zorin-wayland.desktop`.
- `libmutter-14.so.0` (`strings`): no `ext_session_lock` interface; the
  registry list in `design/top-bar-spec.md` (lines 49 to 62) has none either.
- GPUI at the workspace's revision (`TimLai666/zed` `f05b2a97`, the source
  cargo checked out): `WindowKind` (`crates/gpui/src/platform.rs` lines 2000
  to 2027) has `LayerShell` and no session lock kind; no file under
  `crates/gpui_linux/src` names `ext_session_lock`; text input always sets
  `ContentPurpose::Normal` (`crates/gpui_linux/src/linux/wayland/client.rs`
  line 433); `Window::capslock()` exists (`crates/gpui/src/window.rs` line
  2797); `wayland-protocols 0.32.9` is built with its `staging` feature, which
  carries `ext_session_lock_v1`. The `gpui-fast` checkout under
  `~/.cache/better-os/orch/gpui-fast/repo` has no session lock either.
- Ticket 201's VM journal
  (`/home/tim/Developer/better-os-evidence/201/ubuntu/out/journal-full.txt`
  lines 2614 to 2622): installing `greetd` adds the system user `_greetd`
  (home `/etc/greetd`, shell `/usr/sbin/nologin`).

Upstream, read with `gh api` or `curl` on 2026-11-21:

- Luma, `SimplyLuma/luma` at `e5c7e21fb5b4212b5ca4641bb3e50e9113e5af45`
  (a blobless clone outside the repository): `LICENSE.md`, `NOTICE`,
  `src/luma-greeter/` (`README.md`, `LICENSE.md`, `luma-greeter.c`,
  `luma-greeter.css`, `luma-greeter-session.c`,
  `luma-presence-compositor.c`), `config/desktop/dconf/db/gdm.d/00-prairie-login`,
  and the GNOME Shell patches that draw the desktop's login and lock screen:
  `patches/gnome-shell/0002-prairie-login-lock.patch`,
  `0011-luma-presence-login.patch`, `0227-luma-poster-login.patch`,
  `0229-luma-poster-vm-layout-and-avatar.patch`,
  `0236-luma-poster-ultrawide-and-monitor-backgrounds.patch`,
  `0290-compact-login-action-bar.patch`, and
  `patches/gnome-mobile/0003-enable-screen-shield-under-greetd.patch` and
  `0009-skip-gdm-lock-in-greetd-bakeoff.patch`.
- Luma's site: `https://simplyluma.com/try`, `/gallery`, `/blog/alpha`,
  `/blog/luma-and-ai`, `/blog/app-contest`, `/why`, `/download`, read with
  headless Chrome.
- greetd, `kennylevinsen/greetd` (GitHub mirror of
  `git.sr.ht/~kennylevinsen/greetd`) at tag `0.9.0` and at `master`
  `d5f27c3a`: `man/greetd-ipc-7.scd`, `man/greetd-5.scd`,
  `greetd/src/context.rs`, `greetd/src/server.rs`, `greetd/src/config/mod.rs`.
- Debian's packaging, `salsa.debian.org/debian/greetd` at tag
  `debian/0.9.0-4`: `debian/rules`, `greetd.install`, `greetd.postinst`,
  `greetd.postrm`, `greetd.greetd.pam`, `greetd.greetd-greeter.pam`,
  `patches/config_tweaks.patch`, `patches/systemd_service_tweaks.patch`, and
  the upstream `greetd.service` and `config.toml` there. Ubuntu's
  `0.9.0-4ubuntu1` changes only the build (Launchpad's changelog).
- `pop-os/cosmic-comp` at `0681eaf6` (2026-11-20):
  `src/wayland/handlers/session_lock.rs`, `src/backend/render/mod.rs` lines
  1085 to 1112, `src/state.rs` lines 708, 744, 776,
  `src/wayland/protocols/session_lock_layer.rs`.
- `pop-os/cosmic-greeter` at `20af0633` (2026-11-12): `cosmic-greeter.toml`,
  `cosmic-greeter-start.sh`, `debian/cosmic-greeter.postinst`,
  `.service`, `.templates`, `.pam`, `.sysusers`,
  `debian/cosmic-greeter-daemon.service`, `daemon/src/main.rs`,
  `src/locker.rs`.
- `cage-kiosk/cage` at tag `v0.1.5`: `cage.c`.
- GNOME Shell at tag `46.0`: `js/misc/loginManager.js` lines 38 to 54,
  `js/ui/main.js` lines 230 to 231, `js/ui/screenShield.js` lines 147 to
  156, `js/gdm/util.js` lines 22 to 30 and 502 to 575, `js/gdm/authPrompt.js`
  lines 73 to 79, `js/ui/unlockDialog.js` line 699.
- GDM at tag `46.2`: `daemon/gdm-manager.xml`, `daemon/gdm-session.xml`,
  `daemon/gdm-manager.c` lines 1155 to 1235
  (`gdm_manager_handle_open_reauthentication_channel`), `libgdm/gdm-client.c`
  lines 37, 395 to 460, `daemon/gdm-local-display-factory.c` lines 55 and
  869 to 894.
- `pop-os/cosmic-comp` `src/shell/focus/order.rs` lines 91 to 177 and
  `src/shell/mod.rs` lines 293 and 323, at `0681eaf6`.
- greetd's licence: `GPL-3.0-only` (`greetd/Cargo.toml`); the size of
  `greetd/src` and `greetd_ipc/src` at 0.9.0: 96,791 bytes of Rust (the
  GitHub tree listing). Better OS is `GPL-3.0-or-later` (ADR 0003).

## 1. The reference: Luma's login and lock screens

### 1.1 Which program draws which screen

Luma has two different programs, one per form factor, and its README for
`luma-greeter` describes only the handheld one.

| Profile | Login screen | Lock screen | Where it is drawn |
| --- | --- | --- | --- |
| Desktop and tablet (wider than 700 logical pixels) | GDM's own greeter: GNOME Shell in GDM's mode, patched by Luma | GNOME Shell's unlock dialog in the person's session, patched by Luma | `patches/gnome-shell/0227-luma-poster-login.patch` and its follow-ups (Luma calls the composition "Poster"); one class draws both ("the same native GDM and unlock actors share this composition", the patch's stylesheet comment) |
| Narrow screens, in GNOME Shell (700 logical pixels or less) | the same, laid out compact | the same, laid out compact | `0290-compact-login-action-bar.patch`: `compactLogin(width, scale)` is `width / scale <= 700` |
| Handheld boot (the FP6 phone image) | `luma-greeter`: C, GTK 4, on greetd, inside phoc or Cage | phosh's lock screen, themed (`patches/phosh/0008-luma-theme-native-lockscreen.patch`, not read) | `src/luma-greeter/` |

So the desktop form the owner saw is GNOME's GDM and unlock dialog,
restyled; greetd is used only on the phone. Luma's GDM-mode patches are
GJS inside GNOME Shell, which ADR 0012 does not allow Better OS to write, and
Better OS's session does not run GNOME Shell at all.

### 1.2 The desktop composition ("Poster")

Measured from the stylesheet and the allocation code of patch 0227, with the
card width from 0236. Logical pixels.

- **Background.** The system wallpaper, full bleed, with an SVG overlay
  darkening it; at login it starts scaled to 103% and settles to 100%. A
  background is drawn on every monitor (0097).
- **The clock group**, bottom left: 64 from the left edge, 56 from the bottom
  (right in right-to-left). Three lines: the weekday in a display size,
  148 px, weight 700, letter-spacing −0.055 em, line height 0.84; under it a
  row with the time at 64 px weight 250 with tabular figures, AM or PM at
  18 px weight 600 and 75% white, a 1 × 36 px divider at 35% white, and the
  date (`%B %-d`) at 22 px weight 500. White, with a 30 px soft shadow. Below
  1100 px of width the weekday is 104 and the time 48 ("compact").
- **The card**, the right edge: inset 20 from the top, right and bottom, as
  tall as the screen allows; width `min(width − 40, clamp(480, 16% of
  width, 720))` since 0236 (it was `clamp(380, 33.4%, 480)` in 0227);
  radius 24; padding 26 / 30 / 22; a glass fill with a 40 px background blur,
  made opaque (`#26262a`) when high contrast or reduced transparency is on.
- **Inside the card, top:** a status row aligned to the end: Wi-Fi icon
  (15 px), a 23 × 10 battery drawn with its fill, a 1 × 18 divider, and a
  32 × 32 power button whose menu holds Sleep, Restart, Shut down (each with
  "Sleeping…", "Restarting…", "Shutting down…" on a full-screen 82% black
  overlay).
- **Inside the card, centre:** the identity and entry, vertically centred in
  the space left: a round avatar 76 px (initial at 30 px weight 650 when
  there is no picture), a greeting 26 px weight 650 ("Good morning, %s",
  "Good afternoon", "Good evening" by the hour) 20 above and 22 below; the
  password well 320 × 44, radius 13, a dark inset fill, a 1.5 px focus ring,
  with a 34 × 34 light submit button inside its end; under it "Or touch the
  fingerprint sensor" with a 15 px glyph at 12.5 px when a reader exists, and
  "Wrong password" at 12.5 px, centred, 8 below.
- **Inside the card, bottom (lock screen only):** "While you were away", an
  11.5 px heading, over a well of rows: a 32 px icon, the title at 13 px
  weight 650, one line of summary at 12 px, the time at 11.5 px tabular,
  rows divided by a 1 px line.
- **Keyboard:** Tab goes field → submit → power and back around.
- **Leaving:** on success the card fades and moves 24 px to the end over
  600 ms (ease-out cubic) and the clock fades and drops 12 px over 400 ms
  (ease-out quad); then GDM starts the session. With animations off, both are
  instant.
- **What it leaves out:** one person only ("Poster has one person; no
  alternate account chooser is displayed", 0227's `_showUserList`); the first
  unlocked account is used. GDM's session menu is hidden and never shown. No
  accessibility menu on the card (GNOME's keyboard shortcuts still work).

### 1.3 The narrow composition (quiet and entry)

From 0290, used at 700 logical pixels or less:

- **Quiet:** only the clock, centred across the width with 16 px insets,
  its centre at half the screen's height; the time's size is
  `clamp(48, width / 4.8, 96)`, the date (`%A, %B %-d`) 17 px weight 400 at
  78% white; no weekday line and no AM/PM.
- **Entry:** reached by a swipe up or a key. As the swipe progresses from 0
  to 1, the clock's centre rises from 50% to 23% of the height, the
  wallpaper blurs up to 32 px (not with high contrast), and the entry rises
  48 px into place while it fades in, occupying the lower 60% of the screen
  with 16 px insets and its card fill removed: a field row 44 tall with an
  "Unlock" button 44 × 44, radius 14, inside a 72% dark pill.

### 1.4 The handheld greeter (`luma-greeter`)

The README's "quiet clock and passcode compositions … one responsive native
component", with "an identity zone between the date and keypad". From
`luma-greeter.c` and `.css`:

- **Process:** runs as greetd's greeter account inside phoc (or Cage), reads
  the person from the account database and the avatar from
  `/var/lib/AccountsService/icons/<user>`; one fixed account and one session
  command compiled in.
- **Quiet (clock) page:** the time 46 px weight 500, the date 13 px weight
  600, 178 px from the top; "Tap to unlock" 36 px above the bottom. A tap,
  Enter or Space goes to the passcode page. The two pages cross-fade in
  260 ms.
- **Passcode page:** the time and date again at the top (50 px padding);
  then the identity zone, at least 176 px tall and growing with the space
  left, centring a 68 px rounded square avatar (radius 15), the name 14 px
  bold 14 below it (at most 24 characters, ellipsized), and four 11 px dots
  12 apart, 20 below; then a 3 × 4 keypad of 88 × 64 round keys 12 apart, a
  feedback line 64 tall, and an "Emergency call" pill 44 tall. Under 760 px
  of height everything tightens (`compact`: identity zone 128, avatar 56,
  keys 52 tall). A drag from the left 36 px edge, at least 64 px to the
  right, clears the digits and returns to the clock; so does Escape.
- **Authentication:** greetd's IPC over `GREETD_SOCK`; exactly one `secret`
  prompt is answered and a second is refused; `info` and `error` messages
  are acknowledged; the four-digit buffer is wiped with `explicit_bzero` on
  every change of page and right after it is copied to the worker. After a
  failure: "Passcode incorrect. Try again.", and the keypad is disabled for
  1, 2, 4, 8, then 16 seconds (`1 << min(failures − 1, 4)`, capped at 30).
  The screen blanks after 30 seconds without input.

### 1.5 What Better OS follows, and what it does not take

Luma's code is Apache-2.0, its documentation CC BY 4.0, and its visual
assets CC BY-SA 4.0 (`LICENSE.md`; `NOTICE`; `src/luma-greeter/LICENSE.md`
and every source file read carry `SPDX-License-Identifier: Apache-2.0`).
Better OS writes its own program and takes no Luma file.

| Followed: the arrangement and the behaviour | Not taken |
| --- | --- |
| The clock group at the bottom start of the screen, a large weekday over a row of time and date; the full-height card at the end edge; status at the card's top, identity and entry centred in it, notifications at its bottom on the lock screen | Luma's wallpapers (`prism.png`, `luma-prism.png`), its SVG overlays and glass images (`poster-*.svg`), its avatar artwork, its Prairie icons and the hand-drawn glyphs in `luma-greeter.c`, and the Figtree font file. Better OS uses its own artwork (L5), `better_ui::icon_set`, and the platform UI font |
| One composition for login and lock | Luma's corner radii (24 and 13), its pill buttons, its glass blur and its colours; Better OS draws `DESIGN.md`'s radii (4 in the card, 8 for the card), its fills and its accent |
| The narrow quiet and entry views, the clock rising to make room, the entry rising 48 px | The time-of-day greeting: zh-TW has no evening greeting that does not read as "good night", so the card shows the person's name |
| The identity group of picture, name and entry between the date and the controls | The numeric keypad and four-digit passcode: a desktop login is a password; a PIN is reserved for later (section 3.10) |
| Leaving with the card moving 24 px to the end over 600 ms and the clock dropping 12 px over 400 ms | Luma's single-person, no-session-menu login: Better OS lists several accounts and offers the session choice |
| The doubling wait after a failure, the wiping of the secret buffer, one secret per prompt unless the module asks again | Luma's emergency call pill (no telephony on a laptop) |

Reference screenshots were looked for and not found: the site's `Try` page
offers a desktop application that runs Luma in a virtual machine
("Luma runs in a virtual machine inside an app on macOS, Windows or Linux"),
not a preview in the browser; the gallery (27 images) and the three blog
posts show desktops and applications and no lock or login screen; the
repository has no image of either. Running the Try application would mean
downloading and running a third-party program on the owner's machine, which
was not done. What was captured is the `Try` page itself, at 1440 × 2400,
in `/home/tim/Developer/better-os-evidence/259/reference/` (the site has no
dark appearance; both files are the same picture). The description above
is therefore from the source only.

## 2. Better OS's screen at a glance

Two layouts by the screen's logical width, the same in the login screen and
the lock screen:

- **Wide** (800 logical pixels and wider): the clock group at the bottom
  start, the card at the end edge.
- **Narrow** (below 800): the quiet view of the clock alone, and the entry
  view with the clock risen and the entry below it.

Luma's threshold is 700; Better OS's is 800 because the card needs 440 plus
the clock group's narrowest useful width (360) plus the insets. A 960-wide
screen is wide (mockup `narrow-960`).

Every number below is in logical pixels; at scale 2 every one doubles in
device pixels, and at the 150% the compositor turns into 2 (ticket 200)
the screen draws at 2 and is scaled down.

## 3. The login screen

### 3.1 Layout (wide)

| Part | Where | Size and type |
| --- | --- | --- |
| Wallpaper | full bleed on every output | Better OS's login artwork (L5); a gradient scrim from 45% black at the bottom start corner to clear at the middle, so white text reads on any picture |
| Clock group | start 64, bottom 56; right-to-left mirrors it | weekday 120/120 weight 600; under it, 16 below, a row: time 56/64 weight 300 with tabular figures, the period (only for a 12-hour clock) 18/24 weight 600 at 75% white, 8 from the time and on the side the locale puts it (「下午 3:42」, "3:42 PM"), a 1 × 32 divider at 35% white 16 either side, the date 20/28 weight 400 at 90% white. White in both appearances, with a 0 2 24 shadow at 25% black. Below 1100 wide: weekday 88, time 44 |
| Card | top 20, end 20, bottom 20 | width 480 at 1100 and wider, 440 below; radius 8 (a surface that floats, `DESIGN.md`); padding 24; fill: the flyout fill at 88% (`#f9f9f9` light, `#2c2c2c` dark) over the wallpaper blurred once at 32 px; 1 px control stroke; shadow 0 8 32 at the flyout shadow colour |
| Status row | the card's top, end-aligned, 32 tall | 32 × 32 subtle buttons, 4 apart: keyboard layout (its short name, Body Strong 12), accessibility, network, battery with its percentage (Caption), a 1 × 16 divider, power |
| Identity group | centred vertically in the space between the status row and the bottom row | avatar 96 round (initial in Title 28 when there is no picture, on a fill derived from the account's name); 16 below it the name in Title 28/36 weight 600, one line, ellipsized; 4 below a Caption line in secondary text (section 3.3) |
| Entry | 24 below the identity group, 320 wide, centred | a Fluent text field 320 × 36: placeholder, the reveal button 28 × 28 and the submit button 28 × 28 in the accent inside its end, radius 4, a 2 px accent bottom line while focused |
| Message line | 8 below the field | Caption 12/16, centred, at most two lines; the critical colour after an error glyph for errors, secondary text with an information glyph for notices |
| Reserved line | 4 below the message line, 20 tall | empty now; holds the fingerprint or smart card hint later (section 3.10), so adding it moves nothing |
| Bottom row | the card's bottom, 32 tall | start: Switch account (people icon and label) when there is more than one account; end: the session button (section 3.6) |

The card is drawn on the primary output only (section 3.13); every other
output shows the wallpaper and the clock group.

### 3.2 The background

Under L5 (a), the login screen's picture is Better OS's own artwork, light
and dark variants following the system's appearance, shipped in the greeter's
package under `/usr/share/better-os/greeter/`. The mockups draw a stand-in
made of CSS gradients; the real artwork is ticket 279's. The greeter reads
it at start and blurs a copy once (GPUI draws no backdrop blur, so the card
is filled from the pre-blurred copy behind it).

### 3.3 One account

The login screen opens in the entry view with focus in the password field,
for the account that logged in last on this machine (AccountsService's
`ListCachedUsers`, ordered by `LoginTime`); on a first boot with one account,
that account.

| State | Caption under the name | Field and messages |
| --- | --- | --- |
| Ready | (none) | placeholder 「密碼」 / "Password"; submit button name 「登入」 / "Sign in" |
| Checking (greetd and PAM working) | 「正在確認…」 / "Checking…" | field and buttons disabled, a 16 px progress ring replaces the submit glyph |

There is no "already signed in" state: greetd shows the greeter only when no
session runs on the seat (section 5.2).

### 3.4 Quiet and entry (L3)

Under L3 (b): the lock screen opens in the quiet view when the screen
wakes, with the card hidden and, centred 40 above the bottom edge, the hint
in Body, white with the clock's shadow: 「按任意鍵或點一下來解鎖」 /
"Press any key or click to unlock". Any key, click or touch shows the entry
view: the card fades in and moves 24 px in from the end over 250 ms (Fluent's
"fast out, slow in"), focus lands in the field, and a printable key that
woke it is the field's first character (Enter, Space and Escape are not
typed). After 60 seconds without input the entry view clears the field and
goes back to the quiet view over 200 ms. The login screen opens in the entry
view and stays there.

Under L3 (a) the card never hides on a wide screen.

Narrow screens always have both views (section 3.14).

### 3.5 Several accounts

When AccountsService lists more than one account, the bottom row shows
Switch account. Pressing it replaces the identity group and entry with the
chooser (mockup `accounts`):

| Part | Copy |
| --- | --- |
| Title (Subtitle 20/28) | 「選擇帳號」 / "Choose an account" |
| A row per account, 48 tall: avatar 32, the name in Body Strong, a Caption line | Caption: 「上次使用」 / "Last used" for the preselected one, nothing for the others |
| Last row | 「其他帳號…」 / "Other account…": shows a user-name field labelled 「帳號名稱」 / "User name" above the password field, for an account AccountsService does not list |
| Back | 「返回」 / "Back" (Escape too) |

Choosing a row returns to the entry view for that account. Accounts are
those AccountsService lists (it leaves out system accounts and hidden ones);
the greeter never reads `/etc/passwd` for a list. The order is the last
login first, then by name.

### 3.6 Session choice

The session button at the end of the bottom row reads
「工作階段：{name}」 / "Session: {name}" and opens a menu (mockup
`session`) listing every `.desktop` file in `/usr/share/wayland-sessions/`
with its `Name` (localized `Name[zh_TW]` when present), the last-used one
(AccountsService's `Session` for the chosen account) checked, and Better OS
first. A GNOME session (its `Exec` runs `gnome-session`, as Zorin's two
entries do: `env GNOME_SHELL_SESSION_MODE=zorin /usr/bin/gnome-session
--session=zorin`) is listed like any other and locks through the GNOME
unlock service (section 5.6, L2 (a)); under L2 (d) it is left out. The
greeter shows a GNOME session only while the unlock service answers on the
system bus, and checks again right before starting it (section 5.6), so a
session that could not lock is never started. The choice is written to
AccountsService (`SetSession`, the account's own data, the same call ticket
205 makes) only after a successful login, so a wrong password changes
nothing, and not by the greeter: changing another account's data needs
`org.freedesktop.accounts.user-administration`, which polkit gives only with
an administrator's password (`auth_admin_keep`,
`/usr/share/polkit-1/actions/org.freedesktop.accounts.policy`), and the
greeter has no way to ask for one. The greeter passes the chosen session and
language in `start_session`'s environment (`BETTER_OS_LOGIN_SESSION`,
`BETTER_OS_LOGIN_LANGUAGE`), and `better-os-remember-login`, started once in
the new session by an XDG autostart entry, writes them as the person
(`change-own-user-data`, allowed without a password).

| Item | zh-TW | English |
| --- | --- | --- |
| Menu title (screen reader) | 選擇工作階段 | Choose a session |
| Better OS | Better OS | Better OS |
| A session with no translation | its `Name` | its `Name` |

### 3.7 Keyboard layout and input method at the prompt

A password is typed in a keyboard layout, never through an input method. The
greeter runs no input method at all: no Fcitx 5, no IBus. Its layouts are
the system's (`org.freedesktop.locale1`'s `X11Layout`, which is
`/etc/default/keyboard`'s `us` on the owner's machine), applied by the
compositor's `xkb_config`; the person's own layouts live in their home and
are not read (section 5.8).

- The status row's layout button shows the current layout's short name
  (「英」 is not used; the XKB short name, `US`), named for the screen reader
  「鍵盤配置：英文（美式）」 / "Keyboard layout: English (US)".
- With one layout the button is informative only. With several, it opens a
  menu of them, and the system's switch key (ticket 94's Alt+Shift, carried
  into `xkb_config`) cycles them.
- On the lock screen, where the person was typing Zhuyin, the lock surface
  takes the keyboard and the input method gets nothing (ticket 201 saw this
  with `cosmic-greeter`: keys went to the locker, not to Fcitx 5). The field
  asks for the `password` content purpose so an input method that does see
  it stays out (L6). When the session's current input source is an input
  method, the message line shows, once per lock:
  「注音在這裡暫停，請用英文鍵盤輸入密碼。」 / "Zhuyin is paused here. Type
  your password with the English keyboard."

### 3.8 Caps Lock

While Caps Lock is on and the field has focus, the message line shows a
notice (mockup `caps-lock`): 「Caps Lock 已開啟」 / "Caps Lock is on", with
the information glyph. It goes away when Caps Lock goes off. GPUI reports the
state (`Window::capslock()`). An error takes the line first; the notice
returns when the error clears.

### 3.9 Wrong password, the wait, a locked account, an expired password

| State | What shows | zh-TW | English |
| --- | --- | --- | --- |
| Wrong password | the field is cleared and keeps focus; the message line in the critical colour | 密碼不正確，請再試一次。 | That password isn't right. Try again. |
| The wait after a failure | PAM's own delay (3 s from `pam_faildelay` in `/etc/pam.d/login`, which greetd's PAM file includes) runs while "Checking…" shows; after the second failure in a row the greeter adds Luma's doubling wait (1, 2, 4, 8, 16, then 30 s each time), the field disabled and the message counting down (mockup `wrong-password`) | 密碼不正確。請在 {n} 秒後再試一次。 | That password isn't right. Try again in {n} seconds. |
| Locked account (`pam_faillock`, `pam_tally`, or an administrator's lock: PAM's `acct_mgmt` fails) | title in the message line, PAM's own text below it in secondary text | 這個帳號目前無法登入。 | This account can't sign in right now. |
| Account expired | as above | 這個帳號已停用，請聯絡系統管理員。 | This account has been turned off. Ask your administrator. |
| Password expired (PAM asks for the current and a new password) | the identity group stays; the field's label becomes the question PAM asks, one prompt at a time; known `pam_unix` prompts get Better OS's copy, any other prompt shows PAM's text as it is | 你的密碼已過期，請設定新密碼。 / 目前的密碼 / 新密碼 / 再輸入一次新密碼 | Your password has expired. Set a new one. / Current password / New password / Type the new password again |
| Password changed | a notice, then the session starts | 密碼已變更。 | Password changed. |
| PAM's informational text (`info`) | the message line, secondary text | PAM's text | PAM's text |
| greetd not answering | the field disabled; the power menu still works | 登入服務沒有回應。請重新開機。 | The sign-in service isn't responding. Restart the computer. |

The greeter never decides that a password is wrong; it shows what greetd
says (`auth_error`). It answers as many `secret` and `visible` prompts as PAM
asks, in order, each with its own field (greetd's manual: "a greeter should
not make any assumptions about the messages"), unlike Luma's handheld
greeter, which refuses a second secret.

### 3.10 Later additions: PIN, fingerprint, smart card

Not built now; the layout reserves what they need so adding them moves
nothing:

- **The reserved line** under the message line (20 tall) holds the hint:
  「或將手指放在指紋感應器上」 / "Or touch the fingerprint sensor";
  「或插入智慧卡」 / "Or insert your smart card".
- **The field** becomes the PIN field with the same size; a PIN is a PAM
  module's business, never the greeter's.
- **What limits it.** greetd runs one PAM conversation at a time for one
  service (`/etc/pam.d/greetd`), so the login screen cannot wait for a
  finger and a password at once the way GDM does with its separate
  `gdm-fingerprint` service; with `pam_fprintd` before `pam_unix` the
  password field would appear only after the reader times out. The lock
  screen runs PAM itself and can hold two conversations (password and
  fingerprint) at once, as GNOME's does. Ticket 278 holds both.

### 3.11 Accessibility

The accessibility button opens a menu of four switches (each a Fluent
toggle, the state remembered for the login screen in
`/var/lib/better-os-greeter/settings.json`, which only the greeter's account
writes, and taken from the person's own settings on the lock screen):

| Switch | zh-TW | English | What it does |
| --- | --- | --- | --- |
| Screen reader | 螢幕閱讀器 | Screen reader | starts Orca (from Ubuntu's archive) as the greeter's account in the greeter's own D-Bus session, with an AT-SPI bus; every control has the name in section 3.15 through AccessKit. Super+Alt+S toggles it from anywhere on the screen, as on GNOME |
| High contrast | 高對比 | High contrast | the wallpaper becomes a solid fill (black in dark, white in light), the card opaque, strokes 2 px, secondary text becomes primary |
| Large text | 大字體 | Large text | every text size × 1.25 (GNOME's `text-scaling-factor` for large text); the card grows to 560 where the screen allows it, else the screen uses the narrow layout |
| On-screen keyboard | 螢幕小鍵盤 | On-screen keyboard | a keyboard drawn by the program itself across the bottom 40% of the primary output, in the current layout, with Caps, Shift, Backspace and Enter; keys go straight into the field, no virtual keyboard protocol is involved. Ubuntu's archive has no `squeekboard` (`apt-cache policy`), and an external keyboard would need the compositor to accept a virtual keyboard from it before login |

Menu title for the screen reader: 「協助工具」 / "Accessibility".

### 3.12 Power, restart, sleep

The power button opens a menu; each item calls logind (`Suspend`, `Reboot`,
`PowerOff`) and is shown only when logind's `Can…` answers `yes` or
`challenge`.

| Item or state | zh-TW | English |
| --- | --- | --- |
| Menu title | 電源 | Power |
| Sleep / its overlay | 睡眠 / 正在進入睡眠… | Sleep / Going to sleep… |
| Restart / its overlay | 重新開機 / 正在重新開機… | Restart / Restarting… |
| Shut down / its overlay | 關機 / 正在關機… | Shut down / Shutting down… |
| Another person is signed in (logind answers `challenge`: the greeter has no polkit agent and cannot give an administrator's password) | 還有其他人登入，現在無法從這裡重新開機或關機。 | Someone else is still signed in, so you can't restart or shut down from here. |
| The lock screen, the person's own session the only one | the item works; a Caption under Restart and Shut down: 未儲存的工作會遺失 | Unsaved work will be lost |

The overlay is a full-screen 82% black fill with the text in Subtitle,
centred, as Luma's.

### 3.13 Several monitors and scale

- The card and the clock group are on the primary output: the built-in
  panel when the lid is open, otherwise the output the compositor lists
  first. Every other output shows the wallpaper and its own clock group, so
  the time is readable on any screen.
- A click or touch on another output moves the card there for this login
  (a person docked with the lid closed sees it on the screen in front of
  them).
- Output arrival and removal follow `better_ui::layer::LayerSurfaces`, as
  every session surface does (ticket 200).
- Scale is the compositor's: the greeter uses the scale the session would
  (for the greeter, `cosmic-comp`'s own default for the panel; the person's
  `outputs.ron` is in their home and is not read).

### 3.14 Narrow screens

Below 800 wide (a 768-wide tablet in portrait, a split screen):

- **Quiet:** the clock alone, centred with 16 insets, its centre at 50% of
  the height: time `clamp(48, width / 4.8, 96)` weight 300, the date
  (「11月21日 星期六」 / "Saturday, November 21") 17/24 at 78% white;
  the hint 40 above the bottom.
- **Entry:** over 250 ms the clock's centre rises to 23% of the height, the
  wallpaper darkens to the scrim's full strength, the status row appears at
  the top edge (16 insets), and the identity group with the field rises 48
  into place in the lower 60%, the field the full width less 32, at most
  400. The bottom row sits 16 above the bottom edge.
- A swipe up on a touch screen does the same as a key, following the finger
  and settling at release (Luma's behaviour; the session's gesture rule, S6).

### 3.15 Keyboard and screen reader

Tab order: field, reveal, submit, Switch account, session, then the status
row from start to end, then back to the field. Shift+Tab reverses. Escape
clears the field; a second Escape on the lock screen goes to the quiet view.
Enter submits. F10 or the menu key opens nothing (there is no menu bar).

| Control | Accessible name (zh-TW / English) |
| --- | --- |
| Screen | 登入畫面 / Login screen; 已鎖定 / Locked |
| Field | 「{name} 的密碼」 / "Password for {name}" |
| Reveal | 顯示密碼 / Show password; 隱藏密碼 / Hide password |
| Submit | 登入 / Sign in; 解鎖 / Unlock |
| Layout | 鍵盤配置：{layout} / Keyboard layout: {layout} |
| Accessibility | 協助工具 / Accessibility |
| Network | 網路：已連線到 {name} / Network: connected to {name}; 網路：未連線 / Network: not connected |
| Battery | 電池 {n}% / Battery {n}%; 電池 {n}%，充電中 / Battery {n}%, charging |
| Power | 電源 / Power |
| Clock group | the date and time spoken in full |

Network and battery are read-only status, read from NetworkManager and
UPower over the system bus; the login screen offers no network menu.

### 3.16 Starting the session, and a session that does not start

| State | What shows | zh-TW | English |
| --- | --- | --- | --- |
| Starting | after greetd's `success` to `start_session`: the card's field and buttons give way to a progress ring and the line, then the card and clock leave as in section 1.2 (600 / 400 ms) and the greeter exits | 正在啟動 {session}… | Starting {session}… |
| Between programs | greetd stops the greeter's compositor and starts the session's; the screen is black until the session's first frame (both are `cosmic-comp`; how long is not measured, ticket 268) | — | — |
| The session ended within 15 seconds of starting | greetd starts the greeter again; the greeter finds its own record of the last start (`/var/lib/better-os-greeter/last-start.json`: account, session, time) and no session of that account left in logind, and shows this over the entry | {session} 無法啟動。 | {session} didn't start. |
| … its body | | 你可以再試一次，或改用其他工作階段登入。 | You can try again, or sign in to another session. |
| … its buttons | accent first | 再試一次 / 選擇其他工作階段 | Try again / Choose another session |

## 4. The lock screen

### 4.1 One design, and what differs

The lock screen is the login screen's wide and narrow layouts with these
differences:

| | Login screen | Lock screen |
| --- | --- | --- |
| Program | `better-greeter`, under greetd, as the greeter's account | `better-lock`, in the person's session, as the person |
| Who | chosen among accounts | the person whose session it is; no Switch account (section 4.4) |
| Picture | Better OS's artwork (L5) | the person's wallpaper, as the session draws it |
| Opens in | the entry view | the quiet view under L3 (b) |
| Card bottom | Switch account, session | media controls, notifications (L4) |
| Caption under the name | none | 「已鎖定」 / "Locked" |
| Submit | 登入 / Sign in | 解鎖 / Unlock |
| Accessibility switches | the greeter's own | the person's (the session's settings), changed only for the lock screen while it shows |
| Power menu | as section 3.12 | as section 3.12, with the unsaved-work caption |
| Authentication | greetd and PAM's `greetd` service | PAM in the lock program, service `better-os-lock` (`@include common-auth` and `common-account`, `pam_gnome_keyring` to unlock the keyring again, as `cosmic-greeter`'s locker does) |

### 4.2 Notifications (L4)

Under L4 (a), the card's bottom shows the notifications that arrived since
the screen locked, newest first, at most three (mockup
`lock-notifications`):

| Part | zh-TW | English |
| --- | --- | --- |
| Heading (Caption, secondary text) | 你離開時收到的通知 | While you were away |
| A row, 56 tall: application icon 32, title in Body Strong, one line of body in Caption, the time in Caption tabular at the end | the notification's own | the notification's own |
| More | 還有 {n} 則通知 | {n} more notifications |

A notification marked private by its sender (the
`x-better-os-private` hint, or a desktop-entry rule the Notifications page
sets) shows its application and 「新通知」 / "New notification" only. The
Notifications page (ticket 238) gets the setting 「在鎖定畫面上顯示通知」 /
"Show notifications on the lock screen", on by default, and under it
「顯示內容」 / "Show their content". Under L4 (b), or with the setting off,
the bottom of the card is empty (mockup `lock-private`). The rows are not
buttons: nothing is opened or answered from a locked screen.

### 4.3 Media controls

While an MPRIS player is playing or paused, a row sits above the
notifications: the cover (40, radius 4), the title in Body Strong and the
artist in Caption, and Previous, Play or Pause, Next as 32 × 32 subtle
buttons named 「上一首」 / "Previous", 「播放」 / "Play", 「暫停」 / "Pause",
「下一首」 / "Next". Heading for the screen reader: 「正在播放」 / "Now
playing". Media keys work while locked, as they do on GNOME.

### 4.4 What the lock screen does not do

- **Switch user.** Under GDM (L1 A) a second login needs GDM's transient
  display, which `cosmic-comp` sessions have never been tested with; under
  greetd (B, C) it does not exist: greetd keeps one session and starts the
  greeter again only when it ends (`greetd/src/context.rs`). So the lock
  screen has no Switch user button; the card's bottom keeps the space.
- **Answer a prompt over itself.** Ticket 201 found polkit asking for an
  administrator's password behind the lock screen (ticket 256). The lock
  screen never shows another program's prompt over itself (it would be the
  perfect place to fake one), and nothing but the lock program draws above
  it except the compositor's on-screen display through
  `cosmic_session_lock_layer_manager_v1`, which `cosmic-comp` offers only to
  clients that are not sandboxed (`src/state.rs` line 776). A request made
  while locked waits for the unlock; ticket 256 removes the one known cause.
- **Open anything.** Notifications, media and the power menu are the only
  controls; there is no camera, no launcher, no quick settings.

### 4.5 Locking, unlocking, and a crash

- The session launcher starts `better-lock` as its own child, inside the
  session's scope, when logind sends `Lock` (ticket 201: a locker started as
  a systemd user unit exits with `NoSessionForPID`), and on the person's
  Lock action and `cosmic-idle`'s idle lock.
- `better-lock` asks the compositor for `ext_session_lock_v1`, gives every
  output a lock surface, and sets logind's `LockedHint` (which
  `cosmic-greeter` never set, ticket 201). On success it unlocks the session
  lock (`unlock_and_destroy`), clears `LockedHint`, and exits.
- **If it crashes,** the protocol keeps the session locked and
  `cosmic-comp` draws nothing where a lock surface was (`render/mod.rs` lines
  1085 to 1112 push no element without a surface), so every output goes
  black. `cosmic-comp` accepts a new lock from a new client once the dead
  one's connection is gone (`session_lock.rs` lines 22 to 30). The launcher
  starts `better-lock` again within a second; after three crashes in 30
  seconds it starts `cosmic-greeter`'s locker instead (installed with the
  session's closure), which also asks for the password. What the person does
  if both fail is in the security review's numbered steps.
- **logind's `Unlock`** (what `loginctl unlock-session` sends) is honoured,
  as GNOME's screen shield honours it (`screenShield.js` lines 151 to 152):
  it is the way back for a person whose lock screen is broken.
- **Who the lock protects against.** The lock keeps out a person at the
  keyboard who does not know the password. It does **not** keep out a
  program running as the same person: logind lets the session's own user
  call `Unlock` without any password (`bus_verify_polkit_async` with the
  session's uid as the user who needs no authorization, systemd v255
  `src/login/logind-session-dbus.c` lines 201 to 221), and such a program
  can also kill `better-lock` and take the compositor's lock in the second
  before the launcher starts it again (`cosmic-comp` accepts a new lock from
  any client once the old one is gone, `session_lock.rs` lines 22 to 30),
  then unlock it. GNOME's lock has the same limit through logind. Closing
  the second path would need `cosmic-comp` to give the lock only to a client
  the launcher hands over, which is a compositor patch (ADR 0039's
  territory) and is not proposed here.

## 5. How it is built

### 5.1 The programs

| Program | Runs as | In | Reaches |
| --- | --- | --- | --- |
| `greetd` 0.9.0 (Ubuntu's archive) | root | `better-os-greeter.service`, started only by the supervisor | PAM, logind, the VT, the greeter's socket |
| `/usr/sbin/better-os-greeter` (a few lines of Rust: environment, then `exec cosmic-comp /usr/libexec/better-os/greeter/better-greeter`) | `_better-greeter` | greetd's greeter session (class `greeter`) | the compositor |
| `cosmic-comp` | `_better-greeter` | the same | DRM and input through logind's seat |
| `better-greeter` (GPUI) | `_better-greeter` | the same | greetd's socket; the system bus, read-only, for AccountsService, logind, NetworkManager, UPower, locale1; its own state directory |
| `better-lock` (GPUI) | the person | the session's scope, child of the session launcher | PAM (`better-os-lock`), the session bus, the compositor's lock |
| `better-os-login-supervisor` (Rust, no GPUI) | root | `better-os-login-supervisor.service`, which **is** the display manager (`display-manager.service` is its alias), `WatchdogSec=10`, `Restart=always` | logind (read), systemd (start, stop, restart of the units named in section 5.10 and nothing else), `plymouth`, `/sys/class/drm` (read), the greeter's heartbeat, its records under `/var/lib/better-os/boot-watch/` |
| `better-os-gnome-unlock` (Rust, no GPUI) | root | `better-os-gnome-unlock.service`, only while the Better OS greeter is the display manager (`Conflicts=gdm.service`) | owns `org.gnome.DisplayManager` on the system bus; runs PAM (`better-os-gnome-unlock`) for the person whose GNOME session asks, over a private connection matched to that session's process (section 5.6) |
| `better-os-remember-login` (Rust, no GPUI) | the person | an XDG autostart entry, once per login, in any session | AccountsService for the person's own account only (`change-own-user-data`, which polkit allows the person without a password): writes the session and language the greeter passed in `start_session`'s environment |

One crate holds everything both programs share and nothing that draws:
`login-screen-core` (ticket 260): the states, the copy, the layout rules
(thresholds, sizes, the card's width, the clock's sizes, the narrow
geometry), the wait after failures, the account order, the session list
parsed from `.desktop` files, the layout indicator, the notification rows.
`better-greeter` and `better-lock` are thin GPUI shells over it.

### 5.2 greetd

The greeter talks to greetd over the Unix socket in `GREETD_SOCK`, which
greetd creates and gives to the greeter's account (`server.rs` line 182).
Each message is a 32-bit length in native byte order followed by that many
bytes of UTF-8 JSON (`greetd-ipc(7)`, the same at tag 0.9.0 and `master`).
Requests: `create_session` (username), `post_auth_message_response`
(response, optional), `start_session` (cmd, env), `cancel_session`.
Responses: `success`, `error` (`auth_error` or `error`, with a description),
`auth_message` (`visible`, `secret`, `info`, `error`).

What the greeter must do:

- Hold a secret in one fixed buffer of bytes it owns (a `zeroize`-backed
  type, never a `String` that may reallocate), wipe it when the request
  carrying it has been written, on every change of view, on Escape, and when
  the program exits; never log it, never put it in an error or a panic
  message. The JSON for `post_auth_message_response` is written into a
  buffer of the same kind.
- Answer every `auth_message` in order or `cancel_session`; never guess.
- After `success` to `start_session`, exit within 5 seconds: greetd sends
  `SIGTERM` after 5 and `SIGKILL` after 10 (`context.rs`, `alarm`).

What it must not do (each is a test or a sandbox rule in ticket 263):

- Run PAM itself; PAM stays in greetd, under `/etc/pam.d/greetd`
  (`@include login` plus `pam_gnome_keyring` and `pam_kwallet5`, Debian's
  file). greetd 0.9.0 names its PAM services itself: `greetd` for the person
  and `greetd-greeter` for the greeter (`server.rs` lines 202 to 214); the
  `service =` key `cosmic-greeter.toml` uses is from 0.10.
- Store a credential, a hash, or a "remember me".
- Open a network socket. `better-greeter` links an HTTP client through the
  toolkit (AGENTS.md); the sandbox forbids `AF_INET` and `AF_INET6` sockets
  and any TCP connect (section 5.9).
- Read anything under a home directory, before or after authentication.
  `cosmic-greeter-daemon` runs as root and `seteuid`s into each account to
  read its COSMIC configuration for the greeter (`daemon/src/main.rs` lines
  18 to 50, 73 to 89); Better OS does not. What the greeter knows of a
  person comes from AccountsService's D-Bus properties (`RealName`,
  `IconFile`, `Session`, `Language`, `LoginTime`) and the avatar file it
  names under `/var/lib/AccountsService/icons/`.

**A greeter that crashes** ends greetd too: when the greeter exits without
having scheduled a session, greetd returns the error "greeter exited without
creating a session" and stops (`context.rs` lines 357 to 360). The unit's
`Restart=always` (`RestartSec=1`, `StartLimitBurst=5` in 30 s, Debian's
`greetd.service`) would start it again up to five times and then give up,
with nothing on screen. Better OS does not rely on that: the login
supervisor owns every restart and every deadline (section 5.10), and
`better-os-greeter.service` has `Restart=no` and `OnFailure=` only
`plymouth-quit.service`. It has no `OnFailure=` to a text login: a first
crash would then start `agetty` on tty1 while the supervisor was starting
its second attempt on the same terminal, and the three attempts would be
bypassed. Only the supervisor (or, if it cannot run, the backstop) decides
the fall back. GDM is never started on its own (L8).

**One session at a time.** greetd keeps one current session and starts the
greeter again only when it ends (`context.rs`): while one person is logged
in on the seat, nobody else can log in on it. GDM can. This is a cost of
L1 B and C.

### 5.3 The compositor before anyone is logged in (L7)

| | `cosmic-comp` (as `cosmic-greeter` runs it) | Cage |
| --- | --- | --- |
| Installed | with the session's closure, already | `cage`, `libwlroots12t64`, `xwayland` from `universe` |
| Started by | `exec cosmic-comp <greeter>`: `cosmic-comp` runs the program it is given once it is up, as the session launcher already relies on (`better-os-session.md`, "The session") | `cage -- <greeter>` |
| Surfaces | layer shell, per output, keyboard exclusive, input regions, fractional scale: ticket 200 proved each with GPUI at this revision | `xdg_shell` only (`wlr_xdg_shell_create`, `cage.c` line 409); no layer shell |
| Several outputs | one surface per output | one window stretched across all outputs (`-m extend`, the default) or only the last one (`-m last`) (`cage.c` lines 205 to 206) |
| Session lock | offered (not needed by the greeter) | not offered |
| Configuration it reads | `$XDG_CONFIG_HOME/cosmic/` of the greeter's account, which is its own state directory, never a person's | none |

`cosmic-greeter` runs exactly this arrangement on Pop!_OS
(`cosmic-greeter-start.sh`: `exec cosmic-comp cosmic-greeter`); on Ubuntu
24.04 and Zorin OS 18 with the pinned closure it has **not** been run
(ADR 0038's C3 kept GDM, so ticket 201 never started greetd). Ticket 268's
first step is to run it.

### 5.4 GPUI (L6, ticket 257)

- **The greeter** opens one `better_ui::layer` surface per output at the
  overlay layer with keyboard interactivity exclusive, as every session
  surface does. Nothing new in GPUI.
- **The lock program** needs a window kind GPUI does not have: a surface
  given to it by `ext_session_lock_v1` for each output, configured by the
  compositor to the output's size, which must be mapped on every output
  before the compositor reports the session locked. The protocol's bindings
  are already compiled into `gpui_linux` (the `staging` feature); the work
  is a `WindowKind::SessionLock` beside `LayerShell` in GPUI's Wayland
  backend, with lock, unlock-and-destroy and the "finished" event (ticket
  261).
- **A password field the input method leaves alone:** GPUI sets
  `ContentPurpose::Normal` on every text input. The field needs `password`
  with the `sensitive_data` hint, and the lock program needs to keep text
  input off entirely while locked (ticket 261).
- **Accessibility:** GPUI exports the tree through AccessKit to AT-SPI; the
  greeter's start script gives the greeter's account its own D-Bus session
  and AT-SPI bus when the screen reader is on.
- **After ticket 257** (ADR 0045, accepted) both programs build on GPUI Kit
  over GPUI Fast from crates.io; ticket 261's work goes to GPUI Fast as a
  pull request, not to Zed, and GPUI Fast has no session lock either (the
  `gpui-fast` checkout under `~/.cache/better-os/orch/gpui-fast/repo`).

### 5.5 Display manager integration on Ubuntu 24.04 and Zorin OS 18

Debian chooses the display manager through one debconf question,
`shared/default-x-display-manager`, shared by every package that registers
a `<package>/daemon_name` template (`gdm3`, `lightdm`, `sddm`,
`cosmic-greeter`). The chosen package's postinst writes the `daemon_name`
into `/etc/X11/default-display-manager`, and on every configure each of them
links `/etc/systemd/system/display-manager.service` to
`/lib/systemd/system/<basename of that path>.service`
(`gdm3.postinst` lines 26 to 58; `cosmic-greeter.postinst` is the same
code). `dpkg-reconfigure gdm3` asks the question again and relinks.

greetd's own Debian package registers no template, installs its unit with
`[Install] Alias=display-manager.service`, and is installed with
`dh_installsystemd --no-stop-on-upgrade --no-start` (`debian/rules`), so
installing it changes nothing; its config runs `agreety` on VT 7 as
`_greetd` (`patches/config_tweaks.patch`).

So Better OS's greeter registers as a display manager the Debian way, as
`cosmic-greeter` does:

- the package `better-os-greeter` ships the units of section 5.10
  (`better-os-login-supervisor.service`, which is the display manager;
  `better-os-greeter.service`, greetd with `--config
  /etc/greetd/better-os-greeter.toml`; `better-os-gnome-unlock.service`;
  `better-os-login-fallback@.service`; the backstop's service and timer),
  the TOML (`vt = 1`, `command = "/usr/sbin/better-os-greeter"`, `user =
  "_better-greeter"`), a sysusers entry, the unlock service's own bus policy
  for `org.gnome.DisplayManager`, a debconf template
  `better-os-greeter/daemon_name` = `/usr/sbin/better-os-login-supervisor`
  (so the alias names the supervisor's unit), and a postinst copying
  `cosmic-greeter`'s for the shared question. The greeter's unit is written
  for Better OS, not copied from Debian's `greetd.service`: Debian's runs on
  VT 7 (`systemd_service_tweaks.patch`) with `TimeoutStopSec=30s`,
  `Restart=always` and `After=… plymouth-quit-wait.service`, each wrong
  here (section 5.10);
- an upgrade of `better-os-greeter` never stops or restarts
  `better-os-greeter.service` (`dh_installsystemd --no-stop-on-upgrade
  --no-restart-after-upgrade` for it): greetd ends the running session when
  it is stopped (`server.rs` lines 261 to 273 call `terminate`, which sends
  the session's process `SIGTERM`, `context.rs` lines 389 to 403). The
  supervisor and the unlock service are `try-restart`ed (the supervisor
  continues from its state file; the unlock service is gone for about a
  second, and a GNOME unlock attempt in that second fails and can be
  retried); the greeter program re-executes itself as every Better OS
  window does (AGENTS.md's update rule);
- choosing it is `debconf-set-selections` of the shared question to
  `better-os-greeter` and `dpkg-reconfigure better-os-greeter`, which writes
  the file and relinks the alias; it takes effect at the next boot, never by
  stopping a display manager that runs a person's session;
- choosing GDM back is the same with `gdm3`, which is also the one command a
  person can type at a text console (`sudo dpkg-reconfigure gdm3`); and a
  `gdm3` upgrade keeps whatever was chosen, because its postinst relinks from
  the file;
- the session's preseed of the question to `gdm3` before `cosmic-greeter` is
  installed stays (ticket 204); `cosmic-greeter`'s postinst then follows the
  file too.

The exact sequence, what is captured first and how it is put back, is ADR
0044's section on installing and removing.

### 5.6 Locking in every session, without GDM (L2)

The owner's requirement: every session Better OS offers can lock, and no
lock depends on GDM. Three invariants hold in every session, and each is
checked by a test named in ticket 268 or 280:

1. **A locked session never shows its content**, not even for a frame,
   whatever crashes.
2. **The lock survives the lock program crashing**: a crash never unlocks.
3. **There is always a way to authenticate**: a person who knows their
   password can always get back in without losing their session.

#### In the Better OS session

`cosmic-comp` implements `ext_session_lock_v1` itself; no display manager is
involved.

- It offers the global to every client that is not sandboxed
  (`SessionLockManagerState::new(…, client_not_sandboxed)`, `src/state.rs`
  line 708).
- While a lock exists, each output draws only the layers marked to show on
  the lock (the `cosmic_session_lock_layer_manager_v1` extension, which
  `cosmic-osd` uses for its on-screen display) and the output's lock
  surface, and then stops: no workspace, window or panel is drawn
  (`src/shell/focus/order.rs` lines 91 to 177, which return before the
  workspace stages). Keyboard focus can only be a lock surface
  (`KeyboardFocusTarget::LockSurface`, `src/shell/mod.rs`).
- When the lock client dies, the lock stays: `shell.session_lock` is cleared
  only by `unlock` (`src/wayland/handlers/session_lock.rs` lines 45 to 57),
  and an output with no lock surface draws nothing at all, so it is black
  (`src/backend/render/mod.rs` lines 1085 to 1112). A new client may lock
  again because the old one's connection is gone (`session_lock.rs` lines
  22 to 30).
- So invariant 1 and 2 are the compositor's own. Invariant 3: the session
  launcher starts `better-lock` again within a second; after three crashes
  in 30 seconds it starts `cosmic-greeter`'s locker; and from a text console
  the person, logged in there with their password, runs `loginctl
  unlock-session`, which `better-lock` honours (section 4.5).

#### In a GNOME session

GNOME Shell 46 locks inside its own compositor: the screen shield and the
unlock dialog are part of `gnome-shell`, so invariants 1 and 2 are GNOME's
own (if `gnome-shell` crashes, the Wayland session ends and the login
screen appears; nothing of the session is shown). What it takes from GDM
is two things, both found in its source at tag 46.0:

- **Whether locking exists at all.** `main.js` lines 230 to 231 create the
  screen shield only if `LoginManager.canLock()`, which reads the `Version`
  property of `org.gnome.DisplayManager.Manager` at
  `/org/gnome/DisplayManager/Manager` on the system bus and requires it to
  be at least `3.5.91` (`loginManager.js` lines 38 to 54). Without an
  answer there is no shield, no Lock action, and logind's `Lock` signal is
  never listened to (the shield connects it, `screenShield.js` lines 147 to
  156).
- **How it checks the password.** The unlock dialog's prompt is in
  `UNLOCK_ONLY` mode (`unlockDialog.js` line 699), which sets
  `reauthenticationOnly` (`authPrompt.js` lines 73 to 79). `ShellUserVerifier`
  then calls `Gdm.Client.open_reauthentication_channel(userName)`
  (`gdm/util.js` line 505). In `libgdm` that is
  `org.gnome.DisplayManager.Manager.OpenReauthenticationChannel(username) →
  address` on the system bus, then a **private D-Bus connection** to that
  address (`G_DBUS_CONNECTION_FLAGS_AUTHENTICATION_CLIENT`), on which it
  makes a proxy for `org.gnome.DisplayManager.UserVerifier` (and
  `…UserVerifier.ChoiceList`) at `/org/gnome/DisplayManager/Session`
  (`gdm-client.c` lines 37, 395 to 460). Over it the shell calls
  `BeginVerificationForUser(service_name, username)` with
  `gdm-password` (or `gdm-fingerprint`, `gdm-smartcard`, `gdm/util.js` lines
  28 to 30), `AnswerQuery`, `Cancel`, and listens for `Info`, `Problem`,
  `InfoQuery`, `SecretInfoQuery`, `ConversationStarted`,
  `ConversationStopped`, `ServiceUnavailable`, `Reset`, `VerificationFailed`
  and `VerificationComplete` (`daemon/gdm-session.xml`). An
  `AccessDenied` reply is not a fallback in this mode: with
  `reauthenticationOnly` the shell reports "Authentication error" and
  never unlocks (`gdm/util.js` lines 509 to 520).

In GDM the private address belongs to a `gdm-session-worker` it starts as
root for that user, which runs PAM; for a session GDM did not start it
still does this, through a "temporary reauthentication channel", after
finding the caller's logind session from its process
(`gdm-manager.c` lines 1155 to 1235). GNOME needs that answer, not GDM.

The options weighed (L2):

| | How | Invariants | Depends on GDM | Verdict |
| --- | --- | --- | --- | --- |
| **(a) A GNOME unlock service** | `better-os-gnome-unlock`, a Rust system service, owns `org.gnome.DisplayManager` while the Better OS greeter is the display manager, and answers exactly: the `Version` property (`"46.2"`, the installed GDM's version, so the shell's check is the one it was written for); `OpenReauthenticationChannel(username)`; `RegisterSession(details)` (accepted and recorded; the shell calls it at start, `registerSessionWithGDM`); and refuses everything else (`OpenSession`, `RegisterDisplay`) with `AccessDenied`. For `OpenReauthenticationChannel` it finds the caller's logind session from its process, refuses unless the caller's user is `username` and the session is local and on a seat; then it listens on a fresh socket as GDM does (directory `/run/better-os-gnome-unlock/` 0711, socket 0666, `daemon/gdm-dbus-util.c` lines 84 to 107), accepts one connection whose peer (`SO_PEERCRED` and the D-Bus `EXTERNAL` credentials) has the caller's uid and pid, and serves `UserVerifier`, `UserVerifier.ChoiceList` (empty) and `EnableExtensions` on it, running PAM itself for the service names `gdm-password`, `gdm-fingerprint` and `gdm-smartcard` mapped to Better OS's own PAM files (`better-os-gnome-unlock`, `-fingerprint`, `-smartcard`; any other name is `ServiceUnavailable`) | 1 and 2 are GNOME's; 3: the service is restarted by systemd within a second, the shell opens a new channel at the next attempt, and `loginctl unlock-session` from a text console unlocks (the shield listens to logind's `Unlock`) | **No**: the interface is GDM's, but nothing of GDM runs; the service ships its own bus policy for the name (the same rules as `gdm3`'s `gdm.conf`), so it does not depend on `gdm3` being installed | **Recommended** |
| (b) Lock by leaving the session | logind's `Lock` makes a Better OS helper switch the seat to a lock screen on another virtual terminal, and back after authentication, as LightDM's `dm-tool lock` | **Fails 1**: Ctrl+Alt+F*n* back to the session's terminal shows it, unless the session itself hides its content, which is GNOME's own lock again; greetd cannot hold a second session, so it also needs a display manager of Better OS's own (L1 C) | No | Rejected |
| (c) GDM running for GNOME's lock only | GDM's daemon would answer the shell | GDM's local display factory always manages `seat0` and starts its greeter there (`gdm-local-display-factory.c`), so it cannot run beside the Better OS greeter; and it is a dependence on GDM in the owner's sense | **Yes** | Not possible, and excluded |
| (d) GNOME not offered at the Better OS login screen | the session menu leaves out `gnome-session` entries | nothing to lock | No | The cost of doing nothing else; the owner has not asked for it |

What (a) costs and what could break:

- **A second root service that runs PAM.** Its boundary is "the caller's
  own account, from a local session on a seat", checked from the bus's and
  the socket's credentials; that is stricter than GDM, which checks neither
  (`gdm-manager.c` lines 1155 to 1235). It is **not** a boundary against
  another program of the same person: such a program can start its own
  `/usr/bin/gnome-shell` or check its own password with `unix_chkpwd`
  anyway, and can unlock its own session through logind (section 4.5). It
  limits tries itself: after five failures for a uid it waits 1, 2, 4, 8,
  then 30 seconds between attempts, across all three service names, and
  each PAM conversation runs on its own thread with a 60-second limit, so a
  hung `pam_sss` stalls only that conversation. The security review covers
  it.
- **The shell asks only once.** `main.js` calls `canLock()` once, when
  `gnome-shell` starts, synchronously with GDBus's default 25-second
  timeout (`loginManager.js` lines 41 to 47); there is no activation file
  for the name. A GNOME session started while the service is absent or
  hung has no screen shield for its whole life. So the service is
  `Type=dbus` with `BusName=org.gnome.DisplayManager`, the greeter's unit is
  ordered after it, and the greeter checks `Version` again right before
  `start_session` for a GNOME session and refuses to start GNOME (saying
  "GNOME can't lock right now, so it wasn't started. Choose another session
  or try again." / 「GNOME 目前無法鎖定，所以沒有啟動。請選擇其他工作階段，或再試一次。」)
  when it does not answer within 2 seconds.
- **It follows an interface GDM owns.** A GNOME or GDM update can change
  it (a new method the shell calls, a changed path). Ticket 280's test runs
  the installed `gnome-shell`'s own lock and unlock against the service in
  a VM, and runs again on every `gnome-shell`, `gir1.2-gdm-1.0` or
  `libgdm1` version Ubuntu or Zorin ships. When `libgdm1`'s major version
  is not the one it was tested with, the service keeps serving every GNOME
  session already running (a restart during an upgrade never locks a
  person out) and answers `Version` with an error only for sessions that
  start afterwards, so the greeter stops listing GNOME (section 3.6) rather
  than offer a session that cannot lock.
- **Never at the same time as GDM.** It is `Conflicts=gdm.service` and runs
  only while the Better OS login screen is the display manager; switching
  back takes effect at the next boot (section 6), so a running GNOME
  session never loses it.
- **GNOME's "Switch user" button.** GNOME 46 shows it on the lock screen
  when AccountsService says the seat can switch and there is more than one
  account (`unlockDialog.js` lines 858 to 862), and pressing it calls
  `Gdm.goto_login_session_sync` (line 839), which needs GDM to open a
  second login. greetd cannot open one. What the button does then is
  ticket 280's check; the service answers `LocalDisplayFactory` calls with
  an error, and if the button is shown the design's choice is to hide it
  through GNOME's own `org.gnome.desktop.screensaver user-switch-enabled`
  default, set system-wide while the Better OS login screen is chosen.
- **Not proven yet**: that GNOME Shell 46 then locks, unlocks with the
  password, honours `loginctl lock-session` and `unlock-session`, and keeps
  the keyring unlocked; that GDM's worker does nothing else the shell
  needs (for example `pam_setcred` with `PAM_REINITIALIZE_CRED`, which the
  service also calls); and how `pam_gnome_keyring` behaves when the PAM
  conversation runs in a root service without the person's
  `XDG_RUNTIME_DIR` (the service passes the session's environment from
  logind, `XDG_RUNTIME_DIR` and `DBUS_SESSION_BUS_ADDRESS`, as GDM's worker
  does). All of it is ticket 280's VM proof, before any person runs it.

### 5.7 Language

The greeter speaks the system's language (`locale1`'s `LANG`, zh_TW on the
owner's machine) until an account is chosen, then the account's
(AccountsService `Language`) when it is a language Better OS ships. Dates and
times follow `LC_TIME` as GDM's do; the owner's machine has `LC_TIME=lzh_TW`,
whose weekday and month names were not checked (ticket 260 tests the
formatter against it).

### 5.8 What the greeter reads

| What | From | Why |
| --- | --- | --- |
| Accounts, names, avatars, last session, language, last login | AccountsService over the system bus; avatar files under `/var/lib/AccountsService/icons/` | the identity group and the chooser |
| Sessions | `/usr/share/wayland-sessions/*.desktop` | the session menu |
| Layouts | `org.freedesktop.locale1` | the layout indicator and `xkb_config` |
| Power capabilities | logind | the power menu |
| Network and battery | NetworkManager and UPower, read-only | the status row |
| Its own settings and records | `/var/lib/better-os-greeter/` (owned by `_better-greeter`, 0700) | accessibility switches, the last start |
| Artwork, fonts | `/usr/share/` | drawing |

Nothing else. An avatar is decoded with the `image` crate under size limits
(at most 1 MiB and 1024 × 1024), in the greeter's sandbox.

### 5.9 Sandboxing

Both programs restrict themselves at start, before reading anything a person
controls (ticket 263): Landlock (ABI 4 on Ubuntu 24.04's 6.8 kernel and
newer) limiting the file system to the table above, read-only except the
state directory and `$XDG_RUNTIME_DIR`, and denying TCP bind and connect; a
seccomp filter refusing `socket()` for `AF_INET`, `AF_INET6` and
`AF_PACKET` and refusing `ptrace`, `process_vm_readv` and `execve` (except
Orca's start, which goes through the start script before the filter is
applied). The greeter also gets `PR_SET_DUMPABLE` 0 so its memory never
reaches a core file. The security review states what each rule buys.

### 5.10 The login screen always appears (L8)

The owner's requirement: a boot that stays on the splash and never shows a
login screen (ticket 139) must not happen on the Better OS path. Ticket
139's evidence: in the two stuck boots `gdm.service` started and no greeter
session was ever opened, nothing from `gdm-launch-environment` or
`gdm-wayland-session`; every good boot opens the greeter about 4 seconds
after `gdm.service` starts; the cause is still unknown. ADR 0033's watch
restarts a hung GDM once. Ticket 256 adds a logind that crashed at logout
and left no greeter. Two facts read for this design explain why such a
boot stays on the *splash* rather than showing anything: `gdm.service`
quits Plymouth itself and only `OnFailure=` makes systemd do it, so a GDM
that is active and silent leaves the splash up for ever; and GDM 46 waits
10 seconds for `seat0` to become graphical and then forces an X11 attempt
(`gdm-local-display-factory.c` lines 55, 869 to 894). Neither is shown to
be ticket 139's cause, and this design does not claim to fix it.

#### The units, and what starts what

Revised on 2026-11-22 after review: the first version made the supervisor
`PartOf=` the greeter's unit, so every retry and the fall back would have
stopped or restarted the supervisor itself (systemd.unit(5): "When systemd
stops or restarts the units listed here, the action is propagated to this
unit"), and it let the greeter's unit start before the supervisor had
decided anything. Now:

| Unit | Role | Started by | Key settings |
| --- | --- | --- | --- |
| `better-os-login-supervisor.service` | **the display manager**: `display-manager.service` is an alias of it (the Debian `daemon_name` is `/usr/sbin/better-os-login-supervisor`, section 5.5) | `graphical.target`, through the alias | `Type=notify`, `WatchdogSec=10`, `Restart=always`, `RestartSec=1`, `StartLimitIntervalSec=60`, `StartLimitBurst=5`, `OnFailure=better-os-login-backstop.service`; no `PartOf=`, `BindsTo=` or `Requires=` on any unit it controls |
| `better-os-greeter.service` | greetd with the Better OS greeter | **only** the supervisor, after stages 1 to 3 | `Restart=no`, `TimeoutStopSec=4`, `SendSIGKILL=yes`, `Conflicts=getty@tty1.service`, `After=better-os-gnome-unlock.service`, `OnFailure=plymouth-quit.service` and nothing else; **no** `After=plymouth-quit-wait.service` (Debian's and upstream's `greetd.service` have it, and `plymouth-quit-wait.service` is `plymouth --wait` with `TimeoutSec=0`: copied here, the greeter would wait for a splash only the supervisor quits); no `[Install]` |
| `better-os-gnome-unlock.service` | the GNOME unlock service | `Wants=` from the supervisor | `Type=dbus`, `BusName=org.gnome.DisplayManager`, `Restart=always`, `Conflicts=gdm.service` |
| `better-os-login-fallback@.service` | `agetty` with the fall back's message, on the VT named by the instance | only the supervisor or the backstop | `Conflicts=getty@%i.service` |
| `better-os-login-backstop.service` | the last guard, with no Better OS code in it | the supervisor's `OnFailure=`, and `better-os-login-backstop.timer` (`OnActiveSec=90` from the supervisor's first start) | `ExecCondition=` that the greeter never reported `ready` this boot (`/run/better-os-greeter/ready-seen` absent) and no fall back is running (`/run/better-os/login-fallback.active` absent); then only standard programs: `plymouth quit`, `systemctl --no-block start better-os-login-fallback@tty2.service`, `chvt 2` |

The supervisor keeps T0, the attempt count and the stage reached in
`/run/better-os/login-path.json` (rewritten atomically after each step) and
reads systemd's `ActiveEnterTimestampMonotonic` of its own unit's first
start this boot, so a supervisor restarted by its watchdog continues where
it was and its deadlines do not start again.

#### The property

> Within **80 seconds** of the display manager starting (at boot, and every
> time the login screen should come back: after a logout or a session that
> ended), either the Better OS login screen is accepting input, or a text
> login is accepting input on a virtual terminal that is on screen,
> Plymouth has quit, and the journal holds one line
> `BETTER_OS_LOGIN_STAGE=<stage> RESULT=<ok|retried|fell-back> REASON=<…>`
> for every stage that did not pass. If the supervisor itself cannot run,
> the backstop gives the same text login within **90 seconds**.

Why 80 (the first version said 60 and counted stopping a unit as free; the
review found greetd's own unit has `TimeoutStopSec=30s` and that a process
that ignores `SIGTERM` adds the whole stop timeout): good boots show GDM's
greeter about 4 seconds after the display manager starts (ticket 139). The
budget is now written with every stop in it: 12 s for the device, the seat
and logind; three attempts at the greeter of 15 s each; between attempts and
before the fall back a stop of at most 6 s (`SIGTERM`, 4 s, `SIGKILL`, 2 s
for the kernel to reap); 3 s to start the text login: 12 + 15 + 6 + 15 + 6
+ 15 + 6 + 3 = 78, rounded to 80. Plymouth's commands run inside the first
12 s and have their own 5 s limit. "Accepting input" is defined, not
guessed: the greeter writes `ready` to `/run/better-os-greeter/alive` (and
creates `ready-seen`) only after its first frame is presented on every
output and its password field has keyboard focus, and then rewrites the
file every 5 seconds.

#### Stage by stage

T0 is the moment the supervisor (the display manager) starts, or, for a
later appearance, the moment the supervisor sees that no session is running
on the seat any more (stage 9).

| Stage | How it can fail or hang | How the supervisor sees it (a deadline) | What it does |
| --- | --- | --- | --- |
| 1. A DRM device and its driver | no `/dev/dri/card*`; only the firmware framebuffer (`simpledrm`) while the real driver loads late or never; a card with no connected screen | polls `/sys/class/drm` every 250 ms (no udev monitor, so no `AF_NETLINK`); waits until a card from a real KMS driver has a connected connector, or until **T0 + 12 s** | a real card with a screen: continue. Only `simpledrm` at 12 s: continue on it and record it. A real card with no connected screen at 12 s (a closed lid with no monitor yet): start the greeter anyway, so it appears as soon as a screen is connected, and record "no screen"; the property cannot be observed without a screen and is suspended until one is connected, which starts a new T0. No card at all: fall back |
| 1a. The real driver binds later | `simpledrm`'s card disappears and the real one appears | the same poll | restart the greeter on the new card **only while no person's session runs on the seat** (stage 9's test); never stop a person's session; otherwise the next appearance uses it |
| 2. `seat0` becomes graphical | logind never sets `CanGraphical` | logind's property, same 12 s | false at 12 s with a card: continue and record it; a compositor that then cannot open the device fails stage 6 |
| 3. logind answers | crashed, restarting (ticket 256) or hung | every call has a 2 s timeout; an answer by **T0 + 12 s** | never: fall back; a text login works without logind (`pam_systemd` is `optional` in `common-session`) |
| 4. Plymouth | holds the DRM master and the splash; hangs; never told to quit | the supervisor runs every `plymouth` command with a 5 s limit | `plymouth deactivate` before starting the greeter's unit; `plymouth quit --retain-splash` at `ready`; `plymouth quit` on fall back; `plymouthd` killed when a command does not return. The supervisor's unit has no network namespace (`PrivateNetwork=` would hide `plymouthd`'s abstract socket `/org/freedesktop/plymouthd`, in `libply-boot-client.so.5`; abstract sockets belong to a network namespace, network_namespaces(7)); the network is closed with `IPAddressDeny=any` instead |
| 5. The greeter's unit and its session | greetd fails to start, cannot open the VT, the greeter's PAM session fails (account `_better-greeter` missing, locked or expired), greetd hangs | each attempt starts when the supervisor starts the unit; a logind session of class `greeter` owned by `_better-greeter`, or (when logind does not answer) a second process in the unit's cgroup, **5 s** into the attempt | the account is not repaired at boot (`/etc` stays read-only to the supervisor); a missing or unusable account is recorded and the attempt fails, and Better Manager repairs it from the person's next session (ticket 266's health check). A miss ends the attempt |
| 6. The compositor | `cosmic-comp` crashes at start, cannot open the device, the GPU hangs, it ignores `SIGTERM` | the greeter's heartbeat file **10 s** into the attempt | a miss ends the attempt: stop the unit (`TimeoutStopSec=4`, then `SIGKILL`); the second attempt is a plain start, the third in **safe mode** (only the first card, the compositor's software rendering if it has one, unverified, no fractional scale); after the third: fall back |
| 7. The greeter accepting input | crashes, panics, hangs before its first frame | `ready` **15 s** into the attempt; afterwards a heartbeat never older than 30 s while no person's session runs | as stage 6 (three attempts in all per appearance); a stale heartbeat after `ready` with no person's session starts a new appearance |
| 8. PAM, once a password is typed | a module that hangs (`pam_sss`) | the screen is up, so the property holds; the greeter gives greetd 60 s, then `cancel_session` and "The sign-in service isn't responding." | the person can try again or use the power menu |
| 9. A session ends | the session ends and logind crashed at logout (ticket 256) | a person's session leaving the seat, from logind; and, because logind may be the thing that failed, from greetd's own cgroup: while a person's session runs, greetd has a session worker process in `better-os-greeter.service`'s cgroup and no greeter heartbeat is fresh; when that worker is gone and no heartbeat comes within 15 s, the session has ended whatever logind says | a new appearance from stage 1 |
| F. Fall back | — | — | stop the greeter's unit (at most 6 s, as stage 6); `plymouth quit`; give tty1 back to text (`KDSETMODE KD_TEXT` and `VT_SETMODE VT_AUTO` on `/dev/tty1`, which a killed compositor or `plymouthd` may have left in graphics mode); start `better-os-login-fallback@tty1.service` and `chvt 1`; if the greeter's unit has not stopped in 6 s (a process in uninterruptible sleep), start the fall back on **tty2** instead and `chvt 2`, without waiting; write `/run/better-os/login-fallback.active` and the record. The text login accepts input within 3 s |

The text login's message, written by the supervisor into
`/run/better-os/login-fallback.issue` with the stage and the time filled in
(mockup `text-fallback`). On the console it is **always the English
column**: the Linux text console draws with a font of at most 512 glyphs
and cannot show Chinese characters. The zh-TW column is what Better Manager
shows for the same record (section 6), and what `better-os-login-status`
prints in a terminal that can draw it (it prints English when
`TERM=linux`).

| zh-TW | English |
| --- | --- |
| Better OS 登入畫面無法顯示（{stage}，{time}）。 | The Better OS login screen couldn't be shown ({stage}, {time}). |
| 請在下方輸入帳號名稱和密碼登入文字模式。 | Sign in below with your user name and password to use text mode. |
| 登入後輸入 better-os-login-status 查看原因。 | After signing in, type better-os-login-status to see why. |
| 要改回 GNOME 登入畫面，輸入 sudo dpkg-reconfigure gdm3 並選擇 gdm3。 | To go back to the GNOME login screen, type sudo dpkg-reconfigure gdm3 and choose gdm3. |

| Stage name in the message | zh-TW | English |
| --- | --- | --- |
| 1 | 找不到顯示卡 | no graphics device |
| 3 | 登入管理服務沒有回應 | the login manager didn't respond |
| 5 | 登入服務無法啟動 | the sign-in service didn't start |
| 6 | 顯示伺服器無法啟動 | the display server didn't start |
| 7 | 登入畫面程式沒有回應 | the login screen program didn't respond |
| supervisor | 登入監督程式無法執行 | the login supervisor couldn't run |

The backstop writes the last row; it has no stage of its own to report.

#### Automatic login

GDM's automatic login (`AutomaticLoginEnable` in `/etc/gdm3/custom.conf`)
does nothing once GDM is not the display manager. The switch (section 6)
carries it over: it reads GDM's setting, and when automatic login is on it
writes greetd's `initial_session` with the same account and that account's
last session, and the confirmation says so. GDM's file is never edited, so
going back to GDM gives back GDM's own setting. With automatic login, at boot
the property is "a session or a prompt": a session of that account on the
seat within 80 seconds counts.

#### What it cannot promise

- A fault before the display manager starts (firmware, the kernel, the
  initramfs, a disk that does not mount, systemd itself) is outside it; so
  is a machine with no screen at all.
- **A process the kernel cannot kill.** A compositor or Plymouth stuck in
  uninterruptible sleep inside the GPU driver keeps the DRM device. The
  supervisor does not wait for it (the fall back moves to tty2), but whether
  the kernel's text console can draw on a device whose master never let go
  depends on the driver; on such a machine the text login may run and still
  not be visible. Stated, not solved.
- **The supervisor and the backstop both depending on systemd.** If systemd
  itself is stuck, nothing here runs.
- Ticket 139's own cause is unknown. This design does not claim to fix it;
  it makes sure that, whatever the cause, the Better OS path ends in a login
  prompt that says what failed instead of a splash, within the limits above.
- A full disk does not stop the property (the greeter, greetd and the text
  login need no writes; records that cannot be written go to the journal,
  and a journal that cannot write loses them), but it may stop a person's
  session from starting after they log in.

#### How it is proven (tickets 267 and 281)

In the session VM (`packaging/tests/session-vm/`), on Ubuntu 24.04 and Zorin
OS 18, each case boots (or logs out) with one failure injected. The check is
the property, judged **on the VM's own screen**: a screenshot of the screen
the person would see, plus a login typed with the VM's keyboard on that
screen, within 80 seconds (90 for the supervisor's own failures); Plymouth
not running; the expected `BETTER_OS_LOGIN_STAGE` line. A login over the
serial console does not count: it says nothing about what is on screen.

| Injected | How | Expected end |
| --- | --- | --- |
| No DRM device at start | `modprobe.blacklist=virtio_gpu` and `initcall_blacklist=simpledrm_platform_driver_init` | text login, stage 1 (on a console the kernel can still draw, `vga16fb` or serial; recorded which) |
| A late DRM device | `simpledrm` only, then `modprobe virtio_gpu` at 25 s | greeter on `simpledrm` at about 12 s, restarted on the real card while nobody is logged in |
| A late DRM device after login | as above, with a person logged in before the module loads | the person's session untouched |
| A card with no screen | no display attached to the VM's card | greeter started, "no screen" recorded; attaching a display shows it within 80 s |
| A seat that is not graphical | a udev rule removing the `seat` tag from the card | greeter, or text login with stage 2 recorded |
| The compositor crashing at start | a `cosmic-comp` wrapper that exits 1 | two retries, then text login, stage 6 |
| The compositor ignoring `SIGTERM` | the wrapper traps `TERM` and sleeps | each stop ends by `SIGKILL` within 6 s; text login, stage 6 |
| The greeter crashing | `BETTER_OS_TEST_GREETER_PANIC` | as above, stage 7 |
| The greeter hanging | `SIGSTOP` to the greeter after `ready` | restart within 30 s |
| greetd hanging | `SIGSTOP` to `greetd` before it starts the greeter | stage 5 misses, `SIGKILL`, retries, text login |
| logind restarting | `systemctl restart systemd-logind` during the greeter's start | greeter, or text login with stage 3 |
| logind hanging | `SIGSTOP` to `systemd-logind` before T0 | text login, stage 3 |
| logind crashing at logout | `kill -ABRT` of logind as a person logs out (ticket 256) | a prompt within 80 s of the session's end, the end seen from greetd's cgroup; the record names the signal used |
| The greeter's account missing or locked | `userdel _better-greeter`; `usermod -L -e 1 _better-greeter` | text login, stage 5, and Better Manager's health check repairs it later |
| A full disk | fill `/` and `/var` before boot | greeter accepting input; login still possible |
| Plymouth hanging before T0 | `SIGSTOP` to `plymouthd` from an early unit | `plymouthd` killed, greeter or text login |
| The supervisor failing to start | its binary replaced by `false` | backstop: text login on tty2 within 90 s |
| The supervisor crashing repeatedly | `BETTER_OS_TEST_SUPERVISOR_ABORT` at stage 6 | restarted, continues from `/run/better-os/login-path.json`; after five starts, the backstop |
| The supervisor hanging | `SIGSTOP` to the supervisor | watchdog restart within 10 s, continues |
| tty1 left in graphics mode | `SIGKILL` to `cosmic-comp` and `plymouthd` with logind stopped | the fall back's tty shows text (`KD_TEXT` reset) |
| Normal boot | nothing | greeter `ready`, measured (target under 10 s) |

## 6. Installing, switching and removing

Under L1 (A) nothing is installed for the login screen: the lock program
ships in the session's package and replaces `cosmic-greeter` as the
session's locker (ticket 262). Under (B), Better Manager's Better OS session
page gets one more choice once section 9's steps are done (ticket 266). The
switch installs `greetd` and `better-os-greeter` (the greeter, the login
supervisor, the GNOME unlock service, the text fallback), chooses it as the
display manager, and leaves GDM installed and stopped:

| State | zh-TW | English |
| --- | --- | --- |
| Off | 使用 Better OS 登入畫面 | Use the Better OS login screen |
| Its description | 開機和登出後看到的畫面會換成 Better OS 的樣式，鎖定畫面也不再需要 GNOME 登入畫面。同一時間只能有一個人登入。 | The screen you see after starting up or signing out changes to Better OS's, and locking no longer needs the GNOME login screen. Only one person can be signed in at a time. |
| Confirmation button | 改用 Better OS 登入畫面 | Switch to the Better OS login screen |
| On, until the next start | 下次開機時會使用 Better OS 登入畫面。 / 立即重新開機 | You'll see the Better OS login screen the next time you start up. / Restart now |
| On | 正在使用 Better OS 登入畫面。 / 改回 GNOME 登入畫面 | You're using the Better OS login screen. / Go back to the GNOME login screen |
| The last start fell back to the text login (ticket 267's record) | 上次開機時登入畫面無法顯示（{stage}），已改用文字模式登入。 / 查看詳細資料 | The login screen couldn't be shown last time you started up ({stage}), so a text sign-in was used. / See details |
| The greeter needed a retry | 上次開機時登入畫面重新啟動了一次（{stage}）。 | The login screen had to be restarted once last time you started up ({stage}). |
| Automatic login is on in GDM (the confirmation adds) | 你目前設定開機後自動登入 {name}，改用後會繼續自動登入。 | You're set to sign in as {name} automatically. That will continue after the switch. |
| Going back, until the next start | 下次開機時會回到 GNOME 登入畫面。 / 立即重新開機 | You'll see the GNOME login screen the next time you start up. / Restart now |

Both directions are one action and take effect at the next start. Neither
stops anything that runs: stopping greetd ends the person's session (section
5.5), and stopping the unlock service would leave a locked GNOME session
unable to check a password. Going back puts
`/etc/X11/default-display-manager`, the alias link and the debconf value
exactly as they were captured; the next boot then starts GDM, and the
supervisor, the greeter and the unlock service do not start (the unlock
service is `Conflicts=gdm.service`). Purging the packages the switch
installed waits for that boot: Better Manager's next start after it finishes
the removal, as it finishes every pending put-back. Removing the Better OS
session while the Better OS login screen is chosen switches it back first,
and the session's own removal waits for the same boot.

## 7. Not verified

- Nothing here has run. In particular: `cosmic-comp` as the greeter's
  compositor on Ubuntu 24.04 and Zorin OS 18; greetd 0.9.0 starting the
  Better OS session and GNOME; GNOME Shell 46 locking and unlocking through
  `better-os-gnome-unlock` (read from GNOME Shell's and GDM's source, not
  run); whether GDM's session worker does anything more on reauthentication
  that GNOME relies on; every deadline and fall back of section 5.10; the
  black screen between the greeter and the session and how long it lasts;
  `cosmic-comp` having a software renderer for safe mode; GPUI under the
  greeter's account with no person's session around it; AccessKit and Orca
  in the greeter; whether the kernel's text console comes back on tty1
  after a compositor or `plymouthd` that held it in graphics mode was
  killed (the supervisor resets it, section 5.10; ticket 281 checks it);
  what GNOME's "Switch user" button does under the unlock service (ticket
  280); how `pam_gnome_keyring` and a hung `pam_sss` behave in the unlock
  service (ticket 280); whether greetd's cgroup is a reliable sign that a
  session ended when logind has crashed (ticket 281).
- Read from source and settled (no longer open): logind lets the session's
  own user `Unlock` it without a password (systemd v255
  `logind-session-dbus.c` lines 201 to 221); `PartOf=` propagates a stop
  or restart to the unit that names it (systemd.unit(5)); a network
  namespace hides `plymouthd`'s abstract socket (network_namespaces(7),
  `libply-boot-client.so.5`); greetd ends the running session when it is
  stopped (`server.rs` 261 to 273, `context.rs` 389 to 403 at 0.9.0).
- What Luma's screens look like on a screen: no image of them is published,
  and the Try application was not run (section 1.5).
- How the `lzh_TW` time locale renders the weekday line.

## 8. The tickets

| Ticket | What | Blocked by |
| --- | --- | --- |
| 260 | `login-screen-core`: states, copy, layout rules, waits, accounts, sessions, layouts; `DESIGN.md`'s display-size exception | L3, L4, L5 |
| 261 | GPUI: a session lock window kind and the password content purpose | L6, 257 |
| 262 | `better-lock`, the lock screen in the session | 203, 260, 261, 263, 269 |
| 263 | Sandboxing the programs and the review's tests | 260 |
| 264 | `better-greeter`, the login screen on greetd, with its heartbeat; `better-os-remember-login` | 260, 262, 263, 269, 279, L7 |
| 265 | The `better-os-greeter` package: the units of section 5.10, TOML, account, bus policy, debconf registration, upgrade rules | 264, 267, 280, 203 |
| 266 | Better Manager's switch: install, choose, go back, remove, each at the next start; automatic login carried; exact GDM restore | 265, 268, 281, 204, 205 |
| 267 | The login supervisor as the display manager, and the backstop: every stage's deadline, retries, safe mode, Plymouth, the text fallback, records (ADR 0033's watch on this path) | 260 |
| 268 | VM checks on Ubuntu 24.04 and Zorin OS 18, and the way back from a text console done by hand | 265 |
| 269 | The on-screen keyboard in `better-ui` | 260 |
| 278 | Fingerprint and smart card | 262, 264, 280 |
| 279 | Better OS's login artwork, light and dark | L5 |
| 280 | `better-os-gnome-unlock`: GNOME locks without GDM | none (L1 and L2 answered) |
| 281 | The fault-injection suite: section 5.10's property in the VM, every injected failure | 265, 267 |

## 9. The order in which Better OS takes over the login and lock path

Each step is proven before the next starts; nothing reaches a person's
machine before step 5.

1. **The lock screen in the Better OS session** (tickets 260 to 263, 269):
   no change to how the machine boots or to GDM; the session's own lock
   invariants proven in the VM.
2. **GNOME locks without GDM** (ticket 280): in the VM, GNOME started by
   greetd with a test greeter, locked and unlocked through the unlock
   service, the invariants of section 5.6 checked, and `kill -9` of the
   service while locked.
3. **The login path cannot sit silent** (tickets 267, 281): the supervisor
   with a minimal test greeter first, then with the real one, every
   injected failure of section 5.10 ending within 80 seconds (90 when the
   supervisor itself fails) in a login
   prompt.
4. **The Better OS greeter** (tickets 264, 265, 268): every state on both
   bases, and the way back to GDM from a text console done by hand.
5. **Offered as a choice** (ticket 266), first on the owner's machine for a
   week, with every supervisor record read.
6. **The default** for new installs or for everyone: not part of this
   design; the owner decides it after step 5, with its own record.
