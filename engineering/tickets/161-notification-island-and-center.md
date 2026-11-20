# Ticket 161 — A notification island at the top right, and a notification center on the right edge opened by a gesture: design

**Blocked by:** none for the design; building is blocked by 151 and 152 (the
relay and the bar it extends) and by the owner's decisions below

## Under ADR 0038's option D (2026-11-21)

The design (and the owner's N1 to N4) is needed in every option and stays this
ticket's. Building follows ADR 0038's table: in the Better OS session the
island and the center are Better OS's own layer surfaces fed by Better OS's own
`org.freedesktop.Notifications` service (tickets 215 and 216), and the edge
swipe is the scroll pass-through's (ticket 225, after N3); **the GNOME build
(the ADR 0036 amendment for more docks, GNOME's banner suppression, the
relay's notification members) waits** with ticket 151. The design's section
"How it is built on GNOME 46" is therefore written for both, with the GNOME
facts left to the GNOME build when it is taken up; the blocker on 151 above
applies to the GNOME build only.

## Why

On 2026-11-21 the owner asked for two things, after choosing Better OS's own
visual style (ticket 148):

1. A notification island like Luma's (simplyluma.com), at the top right of
   the screen: a new notification arrives as a floating rounded group there.
2. A notification center on the right edge of the screen, as on macOS, that a
   touchpad gesture brings out.

This changes what `design/top-bar-spec.md` section 6.7 and decision D7 (a)
settled the same day: there the clock and bell open a 360px flyout, and "the
banners themselves stay GNOME's: the bar does not draw pop-ups". Ticket 158
builds that flyout; this ticket's outcome replaces or amends it, so settle
this design before 158 is built.

## What is known

- GNOME Shell owns `org.freedesktop.Notifications` and draws its banners at
  the top centre; its list lives in the date menu (on Zorin, in the bottom
  taskbar). Nothing but the shell can draw over the screen on GNOME 46 (ADR
  0036), and the relay ticket 151 builds already reports every notification
  to `better-top-bar` and performs activate, action, dismiss, and clear.
- ADR 0036 lets the relay start exactly one fixed program and make its window
  a dock. An island and a side panel drawn in Rust are more surfaces of that
  kind: dock-layer windows the shell places, which reserve no space and take
  no focus until asked. Whether the same view process may open them, and what
  the relay must do to place them, is an amendment to ADR 0036, not a commit.
- The shell can move its own banner to the top right
  (`Main.messageTray.bannerAlignment`), which keeps GNOME's look. That is the
  fallback if Better OS's own island cannot be built, not what was asked.
- macOS opens its notification center with two fingers moving left from the
  right edge of the trackpad. libinput reports no edge swipe: two fingers
  moving are a scroll, delivered to the window under the pointer. The only
  Better OS code that sees where fingers are on the pad is the scroll
  pass-through (ticket 125, ADR 0030), which holds the pad only while a
  scroll factor other than 1 is set, and whose security review
  (`engineering/specs/touchpad-input-passthrough-security.md`) covers exactly
  what it does today. GNOME's own three- and four-finger swipes reach the
  gesture service without positions, so they cannot tell an edge swipe from
  any other.
- None of this exists on COSMIC yet; ticket 160 carries the bar there after
  ticket 143's decision, and on a layer-shell desktop the island and the
  panel need no shell code at all.

## What to deliver

A design in the same form as `design/top-bar-spec.md`, as a new section of it
or a file beside it, written with whoever holds that spec so that two
sessions never edit it at once:

- **The island.** Where it sits (top right, under the bar, the margin), its
  size, what one notification shows (application icon from the new icon set
  or the application's own, name, title, body, actions, close), how several
  stack, how long one stays, urgency, Do not disturb, fullscreen windows,
  several monitors, pointer and keyboard access, screen reader announcement,
  reduced motion. Every state with zh-TW and English copy, and mockups in
  ticket 148's style, light and dark.
- **The center.** A panel the full height of the work area on the right edge:
  what it holds (the notification list grouped by application, Clear all, Do
  not disturb, and whether the calendar moves here from the flyout), how it
  opens and closes (the gesture, the bell and clock, Super+V and Super+M,
  Escape, a click outside), and that it follows the fingers while the gesture
  is in progress or says plainly that it cannot.
- **One surface or two.** Whether the right-edge center replaces the 360px
  flyout of D7 (a) or lives beside it, with section 6.7, ticket 158, and the
  mockups in `design/top-bar/` brought in line.
- **How it is built on GNOME 46**, with each fact seen in a headless GNOME
  Shell under the sandbox rules before it is relied on: a second and third
  dock window from the view placed by the relay; GNOME's own banners
  suppressed while Better OS draws the island and restored when the service
  goes; what a click outside the panel does to focus. The ADR 0036 amendment
  this needs, in its words.
- **The gesture.** The options with their cost: (a) the two-finger edge swipe
  detected in the scroll pass-through, which means the helper holds the pad
  whenever the gesture is on, a new typed action, an ADR, and the security
  review updated in the same change; (b) a three- or four-finger swipe from
  the existing gesture table, which is not what macOS does and collides with
  GNOME's workspace swipes; (c) no gesture until (a) is decided. A
  recommendation.
- Tickets for the build, cut after the owner decides.

## Decisions that are the owner's

| | Decision | Recommendation |
| --- | --- | --- |
| N1 | Better OS draws the island itself (needs the ADR 0036 amendment), or GNOME's banner is only moved to the top right and keeps GNOME's look. | Draw it: it is what was asked, and the relay already carries every notification. |
| N2 | The right-edge center replaces the clock flyout's notification list, or both exist. | Replace: one list, opened by the gesture, the bell, the clock, and Super+V. The calendar moves to the top of the center. |
| N3 | The gesture: (a), (b), or (c) above. | (a), because (b) cannot tell the edge from the middle of the pad; it is the larger piece of work and comes after the center opens by click and key. |
| N4 | Whether the island and the center exist when the top bar is turned off. | Tie them to the bar component: one card in Better Manager turns all of it on and off. |

## Done when

- The design, its mockups, the verified facts, and the proposed ADR text are
  in the repository, and the owner has answered N1 to N4.
