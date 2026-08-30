# ADR 0010: Better Awake's trigger provider set, fullscreen detection, lid-closed policy, and history retention

## Status

Accepted for Better Awake's rule engine (ticket 26). Ticket 26 does not revisit
these; a later phase that wants to change one comes back here.

## Context

Issue #13 lists nine decisions that must not be made silently. Ticket 25 decided
the first five — the StatusNotifierItem crate, the IPC protocol, the process
split, the icon set, and the preset durations. This ADR decides the remaining
four, all of which block the trigger engine:

- the exact first production trigger provider set,
- whether fullscreen detection requires a minimal GNOME adapter,
- the lid-closed support policy,
- the rule history retention duration.

The pressure behind all four is the same. Issue #13 names eleven trigger kinds,
and a provider that is easy to write is not the same as a provider that is
honest. The rule the issue itself states — an unavailable provider shows an
explanation rather than an inert control — is only meaningful if the shipped set
is a decision rather than an accident of what happened to be readable.

## Decisions

### 1. The first production trigger provider set

Nine of Issue #13's eleven kinds ship as working providers. Two do not, and are
recorded separately below.

| Kind | Source | Cadence |
| --- | --- | --- |
| Application/process running | `/proc/<pid>/comm`, plus `/proc/<pid>/cgroup` for the desktop identifier | poll 5 s |
| AC power connected | `/sys/class/power_supply/*/{type,online}` | poll 10 s |
| Battery percentage range | `/sys/class/power_supply/*/capacity` | poll 10 s |
| External display connected | `/sys/class/drm/*/status`, excluding `eDP`, `LVDS`, `DSI`, and writeback connectors | poll 10 s |
| Audio playback | `/proc/asound/card*/pcm*p/sub*/status`, matching `state: RUNNING` | poll 5 s |
| CPU utilization threshold | `/proc/stat` aggregate line, two-sample delta | poll 5 s |
| Network throughput threshold | `/proc/net/dev` two-sample delta, excluding loopback | poll 5 s |
| Selected interface up | `/sys/class/net/*/operstate`, counting `unknown` as up | poll 5 s |
| Time schedule | `localtime_r` | no I/O |
| Watched file or directory | `inotify` through the `notify` crate | event-driven |
| Fullscreen / presentation state | — | reports unavailable |

The two alternatives considered were a smaller set and a larger one.

A smaller set — power, process, and schedule only, with the rest deferred —
was rejected because the four it would drop are the ones with no workaround. A
user cannot approximate "while a large download runs" from a process name, and
Issue #13's own example menu uses exactly that case.

A larger set, adding a session-bus media-player adapter for audio and a GNOME
Shell adapter for fullscreen, was rejected for this ticket because both are a
desktop-environment dependency. Better Awake is meant to work on Zorin, Ubuntu,
GNOME, and KDE alike, and a provider that only answers on one of them is a
fifth availability state nobody asked for. They are Phase 4's richer desktop
integration.

The audio provider's limits are stated rather than hidden. It reads ALSA, which
every audio server on these desktops ends up using, so it detects real playback
without a daemon or a new dependency. It does not see Bluetooth or network
sinks, and it can lag a stream stopping by the audio server's suspend timeout.
The lag is in the safe direction for a keep-awake rule. The Bluetooth gap is
not, and it is written down in `awake-platform::audio` rather than glossed.

Polling was chosen over `udev` netlink for power and display. Netlink would
deliver charger and monitor events immediately at the cost of a socket, a reader
task, and a reconnect path. Ten seconds of latency on "the charger was
unplugged" is imperceptible; two small file reads every ten seconds is
imperceptible too. The seam is in place if that trade ever changes.

Every interval above is a constant in `awake-platform::provider` with the
reasoning beside it, and a test asserts none of them is sub-second. A provider
no enabled rule needs is not sampled at all, so a machine with no rules does no
trigger I/O whatsoever. The two exceptions are AC power and battery percentage,
which are read on every pass regardless of the rules — see decision 4 of the
safety notes below.

### 2. Fullscreen detection needs a compositor adapter, so it does not ship

Fullscreen and presentation state report unavailable, with the stable key
`awake.provider.fullscreen_needs_compositor_adapter`.

Under X11 a fullscreen window is discoverable from `_NET_WM_STATE`. Under
Wayland there is deliberately no such protocol: a client's fullscreen state is
known to the compositor and to nobody else. The only ways to learn it are a
compositor-specific interface — the GNOME Shell D-Bus introspection API — or the
`org.freedesktop.portal.Inhibit` session-state signal.

Three options were considered.

Shipping an X11-only implementation was rejected. Zorin 18 and Ubuntu 24.04 both
default to Wayland, so it would work on the minority path and silently fail on
the majority one, which is the exact shape of unavailability Issue #13 forbids.

Shipping a GNOME Shell adapter was rejected for this ticket. GNOME Shell's
`Eval` introspection is disabled outside developer mode on current releases, and
the remaining route is a Shell extension — an installed, versioned, separately
breaking artefact that Better Awake has no mechanism to ship or verify.

Reporting unavailable was chosen. The condition still appears in the rule
editor, carrying its explanation, because omitting it entirely would leave a
user hunting for a feature the issue promised. A rule naming it evaluates to
unknown, and unknown never becomes true, so no machine is kept awake by a
condition nothing can answer.

The Portal session-state route is the one worth revisiting. It is a standard
interface rather than a desktop-specific one, and it is already on Phase 4's
list because the Portal inhibitor backend needs it too. It should be decided
alongside that backend, not before it.

### 3. Lid-closed operation is not supported, and is not silently attempted

Better Awake does not offer a lid-closed mode.

The inhibitor locks it takes are `sleep` and `idle`. Neither prevents the
`HandleLidSwitch` action, which logind applies independently, so a session that
is running when the lid closes does not keep the machine up — and Better Awake
never claims it does.

The two alternatives were both rejected.

Taking a `handle-lid-switch` inhibitor lock was rejected on safety grounds. A
closed laptop with the fans against a desk is a thermal question, not a
scheduling one, and Issue #13 itself calls lid-closed an advanced capability
needing explicit thermal warnings. Offering it as a checkbox beside "15 minutes"
would be offering a burn risk beside a timer.

Writing `HandleLidSwitch=ignore` into `logind.conf` was rejected outright. It is
a global, privileged, persistent change to the machine's behaviour that would
survive Better Awake being uninstalled, which is precisely what Issue #13 means
by not implementing normal operation through permanent global settings.

What ships instead is honesty: the effective policy shown to the user lists what
is actually being held, and lid behaviour is not in it. When lid-closed support
is reconsidered, it needs the thermal warning, an explicit per-session opt-in,
and a release path that cannot leave the setting behind — which is a ticket, not
a flag.

### 4. History retention is a bounded count, not a duration

Five hundred entries, in `awake_store::history::MAX_HISTORY_ENTRIES`. The oldest
are dropped first.

A duration was the shape Issue #13 asked about, and it was rejected for now
because no duration can be chosen honestly yet. Thirty days of history is a
handful of entries for one user and thousands for another, and there is no usage
data to pick between them. A count bounds the file whatever the usage pattern,
needs no policy answer, and can hold a duration on top of it later without a
migration.

Unbounded was not an option: the service writes an entry per session for as long
as it is installed.

Five hundred entries is months of ordinary use and a few hundred kilobytes. When
a duration is decided, it applies in addition to this cap rather than instead of
it, and the History view already reports the limit so a missing old session is
explainable rather than looking like a bug.

## Consequences

- Nine providers work and are proved against captured `/proc` and `/sys` trees,
  so a parser is tested against recorded input rather than against whatever this
  machine has installed.
- Fullscreen appears in the rule editor with an explanation and can never keep a
  machine awake. Anything that changes that has to change this ADR.
- Lid-closed rules cannot be written at all, because no condition and no policy
  field expresses one. That is deliberate: an unavailable feature is better than
  a checkbox that silently does nothing.
- The history file is bounded without a retention policy having been decided,
  and the bound is visible to the user rather than implicit.
- Adding a provider means adding a `ProviderKind`, a `Condition` variant, and an
  implementation with a declared cadence and a declared unavailability. That is
  the intended cost, and it is what keeps the shipped set a decision.

## Related

- Issue #13, "Decisions deferred".
- `docs/tickets/26-awake-application-and-rules.md`.
- ADR 0005, the platform boundary these providers sit on.
