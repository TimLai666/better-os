# 43 — Field fixes: Zorin daemon refusal, invisible icons, silent refusals, zbus panic

**Epic:** Field reports from the first real Zorin 18 install
**User Story:** Installing Better Awake from Better Manager on Zorin 18
succeeds instead of being refused by the daemon, the applications grid shows
every Better OS icon, a refusal on screen says what the service actually said,
and the journal stops filling with reactor panics.
**Blocked by:** none

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

## Notes

Carried over from the retired delivery status record.

Ticket 43 is done and released as `v0.2.4`. Like
ticket 42 before it, it was cut from field reports rather than from the backlog,
and from the same real Zorin 18 GNOME Wayland machine — this time running
`v0.2.3`. Four defects, each fixed at its own cause: the daemon refused every
install because it read `VERSION_ID` instead of resolving the Ubuntu base from
`UBUNTU_CODENAME`, the six icons 0.2.3 had just shipped were invisible because
the attribution comment pushed the SVG root past gdk-pixbuf's sniff window, the
refusal on screen dropped the machine detail the service actually gave, and
every window launch panicked with `there is no reactor running`. The record of
what each class is lives in ticket 43`.

The first two of those are worth reading together. The daemon defect was the
*same* os-release misreading ticket 40 fixed in `install.sh`, sitting untouched
in the second place that reads the file; and the icon defect shipped inside the
very release that added the icons, because `verify-deb.sh` asserted that the
file exists and nothing asserted that a loader would recognize it. Both are the
shape of gap to look for first: a fix applied at one call site, and a check that
proves the weaker of two properties.

One thing this release still leaves for someone at a real desktop, and one
thing it closed. The icon defect was reproduced and fixed against the host's own
gdk-pixbuf, so an SVG that renders is now proven rather than assumed — but
nobody has watched GNOME draw the six icons in an applications grid after an
install. And nobody has pressed Escape on Better Launcher yet; that key path is
still untouched by any edit and uncovered by any test.
