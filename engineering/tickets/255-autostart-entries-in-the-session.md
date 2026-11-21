# Ticket 255 — The base system's autostart entries that must not run in the Better OS session stay out

**Spec:** better-os-session.md S4 (the "XDG autostart", "Network secrets" and "Gestures" rows)
**Blocked by:** 203

## Why

Ticket 201 recorded which of the base's XDG autostart entries start in the
session (`*/out/inv1/autostart.txt`). Entries filtered only against GNOME run
here. Three of them collide with what the session provides:

- `nm-applet` (`NotShowIn=KDE;GNOME;`) registers as NetworkManager's secret
  agent and puts an icon in any `StatusNotifierWatcher`. Once ticket 217's
  agent and ticket 156's watcher exist, the person gets two agents (two
  password prompts or a race for one) and a second network icon in the bar.
- `geoclue-demo-agent` (`NotShowIn=GNOME;`) becomes GeoClue's authorization
  agent in the session; under GNOME the shell is that agent and asks the
  person. What the demo agent answers, and whether it asks, was not checked.
- Zorin's `touchegg` client starts and connects to the system `touchegg`
  daemon, which acts on X11 only; with Better Touchpad's gestures in the
  session (tickets 219, 223) a second gesture engine must not be running.

## What to build

- **The rule**: which entries the session keeps out and how, without writing
  the person's home or the system's `/etc/xdg/autostart` (AGENTS.md: the
  session adds nothing that outlives it). Candidates: `systemd-xdg-autostart-generator`
  honours `X-systemd-skip=true` only in the file itself; the session target
  can carry `Conflicts=` on the generated `app-<name>@autostart.service`
  units, which are named from the desktop file and stable; or the session's
  `XDG_CURRENT_DESKTOP` can gain a name those entries exclude (it cannot:
  their `NotShowIn` names GNOME). Pick one, record why.
- The list of excluded entries in one place in the session package with a
  test, and the decision for GeoClue's authorization in the session (an agent
  of Better OS's that asks, or none, which refuses).

## Done when

- [ ] In ticket 201's VM, `nm-applet`, `geoclue-demo-agent`, and Zorin's
      `touchegg` client are not running in the session, still start under
      GNOME, and nothing in the person's home or `/etc` was written for it.
- [ ] Ticket 217's agent is NetworkManager's only secret agent in the session.
