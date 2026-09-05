# 43 — Field fixes: Zorin daemon refusal, invisible icons, silent refusals, zbus panic

**Epic:** Field reports from the first real Zorin 18 install
**User Story:** Installing Better Awake from Better Manager on Zorin 18
succeeds instead of being refused by the daemon, the applications grid shows
every Better OS icon, a refusal on screen says what the service actually said,
and the journal stops filling with reactor panics.
**Blocked by:** none
**Status:** done

## The four defects, each observed on a real install

1. **The daemon refused every plan on Zorin.** Evidence from the field state
   file: `daemon.error.plan_rejected: plan targets release 24.04 but this host
   is 18`. `manager-daemon/src/host.rs` read only `VERSION_ID`, and Zorin 18
   reports `18` there with its Ubuntu base in `UBUNTU_CODENAME=noble` — the
   exact defect `install.sh` fixed for ticket 40, present in the second place
   that reads os-release. The daemon now resolves `UBUNTU_CODENAME` first
   (jammy → 22.04, noble → 24.04), falls back to `VERSION_ID`, and refuses an
   unknown codename rather than degrading to the badge. Five parser tests
   including the verbatim Zorin 18.1 os-release shape.
2. **Every icon rendered as an unrecognized image.** gdk-pixbuf sniffs the
   first bytes of a file to pick a loader, and the attribution comment between
   the XML declaration and `<svg>` pushed the root tag past the sniff window,
   so a valid SVG document was not an image to the desktop. Reproduced with
   `gdk-pixbuf-thumbnailer` against the installed icon and bisected to the
   comment. All six icons now open with `<svg` inside the first 100 bytes and
   the comment inside the root; all six render. `verify-deb.sh` gained a
   sniff-window assertion so the regression cannot ship again.
3. **The GUI hid the refusal reason.** Every `daemon.*` evidence key collapsed
   into one localized sentence and the machine detail was dropped — the field
   report needed a state-file dig to find the actual reason. The failure card
   now shows a Technical detail row with the service's own words, untranslated,
   whenever a detail exists.
4. **A zbus panic on every window launch** (`there is no reactor running`),
   pre-existing and recorded at ticket 42. Cargo unifies features across the
   workspace, several service crates build zbus with its `tokio` flavor, and
   that flavor needs an ambient tokio reactor even through the blocking API.
   `manager-platform`'s dbus client now keeps one process-wide reactor alive
   and enters it before connections and proxies. Verified: an 8-second on-host
   run of the fixed window logs nothing.

## Verification

Workspace gates (fmt, check, test, clippy `-D warnings`); nine host parser
tests; all six icons rendered through the host's own gdk-pixbuf; packaging
build + verify for all eight packages including the new sniff assertion; an
8-second on-host `manager-gui` run with zero stderr where the field install
logged two panics per launch.
