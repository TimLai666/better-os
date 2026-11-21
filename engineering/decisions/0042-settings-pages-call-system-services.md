# ADR 0042: Settings pages call the system's own services; the daemon only where none exists

## Status

Proposed on 2026-11-21 for ticket 229 (`design/system-settings-spec.md`,
decision M6). Accepted when the owner answers M6. On acceptance AGENTS.md's
working rule on privileged mutation gains the sentence under "Decision", and
tickets 237, 241, 243, 246, 247, and 250 may start.

## The owner's decision

| # | Decision | Recommendation |
| --- | --- | --- |
| M6 | (a) A settings page's model calls the system service that owns a machine-wide setting, and that service's polkit action decides; `manager-daemon` gains a method only for a change no service offers. (b) Every machine-wide change goes through its own `manager-daemon` method. | **(a)** |

## Context

AGENTS.md: "Keep privileged system mutation outside GUI and CLI code. It
belongs in `manager-daemon`, reached over D-Bus and authorized by polkit." Every
privileged write Better OS has made so far was one no system service offered:
installing packages, the charge limit, the fan mode, the keyboard's colour,
fstab and udev for drives (ADR 0007, 0017, 0029, 0032, 0034). Where a service
existed, a window already called it directly: power-profiles-daemon and
`gsd-power` (ADR 0017, "Unprivileged controls go through the desktop's own
services"), NetworkManager from the top bar, AccountsService's `SetSession`
from `manager-core` (`engineering/specs/better-os-session.md`, "Switching
means the next login").

The system settings pages of `design/system-settings-spec.md` change settings
that standard services own and authorize themselves, several with an
administrator's password. Read on the owner's machine on 2026-11-21
(`/usr/share/polkit-1/actions/`, `/usr/share/polkit-1/rules.d/`):

| Service | Action | Default (any, inactive, active) |
| --- | --- | --- |
| timedated | `set-time`, `set-timezone`, `set-ntp` | `auth_admin_keep` ×3; Ubuntu's `com.ubuntu.desktop.rules` says yes for an active local member of `sudo` |
| localed | `set-locale`, `set-keyboard` | `auth_admin_keep` ×3; `gnome-control-center.rules` says yes for `sudo` |
| hostnamed | `set-static-hostname`, `set-hostname` | `auth_admin_keep` ×3; the same rule |
| AccountsService | `change-own-user-data` | yes ×3 |
| AccountsService | `change-own-password` | `auth_admin` ×3 |
| AccountsService | `user-administration` | `auth_admin`, `auth_admin`, `auth_admin_keep` |
| NetworkManager | `network-control`, `enable-disable-wifi`, `wifi.share.protected` | active session: yes |
| NetworkManager | `settings.modify.own` | `auth_self_keep`, yes, yes |
| NetworkManager | `settings.modify.system` | `auth_admin_keep` ×3 |
| cups-pk-helper | `org.opensuse.cupspkhelper.mechanism.*` | `auth_admin_keep` for the active session; yes for `sudo` and `lpadmin` under Ubuntu's rule |

The lid and power-button actions are logind's `HandleLidSwitch` and
`HandlePowerKey` in `/etc/systemd/logind.conf`; no service offers a method to
change them.

## Decision

A settings page's GPUI-free model calls the system service that owns the
setting, and the service's own polkit action decides; the desktop's polkit
agent asks for a password when the action requires one (GNOME Shell under
GNOME, `cosmic-osd` in the Better OS session). Better OS never receives,
stores, or forwards a password, and a cancelled authentication is the
control center's "No permission" state. `manager-daemon` gains a method only
for a machine-wide change that no standard service offers, under ADR 0017's
rule (one method and one polkit action per change, closed arguments, the
daemon finds the target itself, writes, and reads back). The first such change
is the lid and power-button drop-in (ticket 247).

The sentence AGENTS.md gains on acceptance: "A change a standard system
service already offers under its own polkit action (timedated, localed,
hostnamed, AccountsService, NetworkManager, cups-pk-helper, BlueZ,
power-profiles-daemon) is made by calling that service from the model crate,
not through `manager-daemon` (ADR 0042)."

## Consequences

- The services' own authorization stays in force, including Ubuntu's rules
  that spare a `sudo` member the password.
- Better OS's reviewed privileged surface grows only by ticket 247's interface.
- A page that calls such a service must handle every polkit outcome
  (`NotAuthorized`, a dismissed dialog, `AuthenticationAgent` missing) with
  the control center's states, with tests against a fake service that answers
  each one.
- A service's behaviour (what it accepts, what it writes) is upstream's; each
  page ticket's VM check is where Better OS learns it.

## Alternatives not taken

- **A `manager-daemon` method per setting.** The daemon runs as root, so when
  it calls timedated or AccountsService the service's polkit check passes for
  root; the daemon would have to re-implement each service's authorization in
  its own actions, and the methods would duplicate the services' interfaces.
- **Running the services' command-line programs** (`timedatectl`, `chpasswd`,
  `lpadmin`) from the page. Each would be a second way into the same service
  with its output format as the interface, and `chpasswd`/`lpadmin` need root.
