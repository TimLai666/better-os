//! Toasts: short notices that float at the bottom right of a window, say one
//! thing, and go away on their own. They never take focus and never block the
//! page, which is what separates them from a dialog. Every Better OS window
//! shows its toasts through this one module.
//!
//! WinUI has no toast control inside an app (`TeachingTip` without a target is
//! the nearest), so the card is built from parts the kit already has: the
//! flyout frame — flyout fill, flyout stroke, 8px corners, a shadow, because it
//! floats — with an info bar's severity icon, a Body Strong title, an optional
//! Body message, at most one action (a link or an undo), and a close button
//! when the notice should be dismissible. Cards stack 16px from the bottom and
//! right edges, at most 480px wide and [`MAX_SHOWN`] at once; the rest wait
//! their turn.
//!
//! The parts:
//!
//! - [`ToastTimer`] — the one timing rule: a toast shows for its duration, the
//!   clock stops while the pointer is on it and resumes with the time that was
//!   left, never less than a second.
//! - [`ToastQueue`] — which toasts show and which wait, with no drawing in it.
//! - [`ToastHost`] — one per window, a GPUI entity holding the queue and the
//!   timer that hides expired toasts. A window keeps one and draws it with
//!   [`toast_layer`] as the last child of its root, above dialogs.
//! - [`Toast`] — the card, also usable on its own (the gallery draws it in the
//!   page).

use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, InteractiveElement, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Task, Window,
    deferred, div, px,
};
use gpui_component::IconName;

use super::Handler;
use super::button::Button;
use super::info_bar::{Severity, severity_icon};
use super::menu::panel_frame;
use super::words::{Action, Locale, action};
use super::{BODY, BODY_STRONG, palette, spacing, text};

/// How long a toast shows when nobody points at it.
pub const TOAST_DURATION: Duration = Duration::from_secs(6);
/// The least a toast keeps once the pointer leaves it.
pub const TOAST_MIN_REMAINING: Duration = Duration::from_secs(1);
/// The widest a toast grows; longer text wraps.
pub const TOAST_MAX_WIDTH: f32 = 480.0;
/// The gap between the toasts and the window's bottom and right edges.
pub const TOAST_MARGIN: f32 = 16.0;
/// How many toasts show at once. The others wait in order.
pub const MAX_SHOWN: usize = 3;
/// Toasts draw above dialogs (a content dialog uses priority 2), so a notice
/// about the dialog is not dimmed by the dialog's own smoke layer.
pub const TOAST_LAYER_PRIORITY: usize = 3;

/// When a toast goes away: after its duration, not counting the time the
/// pointer spent on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToastTimer {
    remaining: Duration,
    /// Since when the clock has been running; `None` while the pointer is on
    /// the toast.
    running_since: Option<Instant>,
}

impl ToastTimer {
    pub fn new(duration: Duration, now: Instant) -> Self {
        Self {
            remaining: duration,
            running_since: Some(now),
        }
    }

    /// How much longer the toast shows, if the pointer stays where it is.
    pub fn time_left(&self, now: Instant) -> Duration {
        match self.running_since {
            Some(since) => self
                .remaining
                .saturating_sub(now.saturating_duration_since(since)),
            None => self.remaining,
        }
    }

    pub fn expired(&self, now: Instant) -> bool {
        self.running_since.is_some() && self.time_left(now).is_zero()
    }

    fn counting(&self) -> bool {
        self.running_since.is_some()
    }

    /// The pointer came onto the toast (`true`) or left it (`false`). The clock
    /// stops while it is there and starts again from where it was, with at
    /// least [`TOAST_MIN_REMAINING`] left so the toast does not vanish under
    /// the pointer as it leaves.
    pub fn hover(&mut self, hovered: bool, now: Instant) {
        match (hovered, self.running_since) {
            (true, Some(_)) => {
                self.remaining = self.time_left(now);
                self.running_since = None;
            }
            (false, None) => {
                self.remaining = self.remaining.max(TOAST_MIN_REMAINING);
                self.running_since = Some(now);
            }
            _ => {}
        }
    }
}

/// Names one toast in a queue.
pub type ToastId = u64;

/// One toast in a queue: what it carries and, once shown, its clock.
#[derive(Clone, Debug)]
pub struct Queued<T> {
    pub id: ToastId,
    pub item: T,
    /// `None` for a toast that stays until it is dismissed.
    pub duration: Option<Duration>,
    pub timer: Option<ToastTimer>,
}

/// Which toasts show and which wait. A timed toast's clock starts when it is
/// shown, not when it is queued.
#[derive(Clone, Debug)]
pub struct ToastQueue<T> {
    next_id: ToastId,
    shown: Vec<Queued<T>>,
    waiting: VecDeque<Queued<T>>,
}

impl<T> Default for ToastQueue<T> {
    fn default() -> Self {
        Self {
            next_id: 1,
            shown: Vec::new(),
            waiting: VecDeque::new(),
        }
    }
}

impl<T> ToastQueue<T> {
    /// Adds a toast; `duration` `None` keeps it until it is dismissed.
    pub fn push(&mut self, item: T, duration: Option<Duration>, now: Instant) -> ToastId {
        let id = self.next_id;
        self.next_id += 1;
        self.waiting.push_back(Queued {
            id,
            item,
            duration,
            timer: None,
        });
        self.promote(now);
        id
    }

    fn promote(&mut self, now: Instant) {
        while self.shown.len() < MAX_SHOWN {
            let Some(next) = self.waiting.pop_front() else {
                break;
            };
            self.show(next, now);
        }
        // A short notice is never starved by notices that stay until closed:
        // when every shown toast is one of those, the next timed toast shows
        // anyway, above the limit, and goes away on its own.
        if self.shown.iter().all(|toast| toast.duration.is_none())
            && let Some(at) = self
                .waiting
                .iter()
                .position(|toast| toast.duration.is_some())
            && let Some(next) = self.waiting.remove(at)
        {
            self.show(next, now);
        }
    }

    fn show(&mut self, mut next: Queued<T>, now: Instant) {
        next.timer = next.duration.map(|duration| ToastTimer::new(duration, now));
        self.shown.push(next);
    }

    /// Removes a toast, shown or waiting. Returns whether it was there.
    pub fn dismiss(&mut self, id: ToastId, now: Instant) -> bool {
        let before = self.shown.len() + self.waiting.len();
        self.shown.retain(|toast| toast.id != id);
        self.waiting.retain(|toast| toast.id != id);
        self.promote(now);
        before != self.shown.len() + self.waiting.len()
    }

    /// Takes away the toasts whose time is up. Returns whether any went.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.shown.len();
        self.shown
            .retain(|toast| !toast.timer.is_some_and(|timer| timer.expired(now)));
        let changed = before != self.shown.len();
        self.promote(now);
        changed
    }

    pub fn hover(&mut self, id: ToastId, hovered: bool, now: Instant) {
        if let Some(timer) = self
            .shown
            .iter_mut()
            .find(|toast| toast.id == id)
            .and_then(|toast| toast.timer.as_mut())
        {
            timer.hover(hovered, now);
        }
    }

    /// How long until the next shown toast's time is up, if any is counting.
    pub fn next_deadline(&self, now: Instant) -> Option<Duration> {
        self.shown
            .iter()
            .filter_map(|toast| toast.timer)
            .filter(ToastTimer::counting)
            .map(|timer| timer.time_left(now))
            .min()
    }

    pub fn shown(&self) -> &[Queued<T>] {
        &self.shown
    }

    pub fn contains(&self, id: ToastId) -> bool {
        self.shown.iter().any(|toast| toast.id == id)
            || self.waiting.iter().any(|toast| toast.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty() && self.waiting.is_empty()
    }
}

/// What a toast says, in the language the window is in when it draws.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToastWords {
    pub title: String,
    pub message: Option<String>,
}

/// A toast's action: a link or an undo. Clicking it also dismisses the toast.
#[derive(Clone)]
pub struct ToastAction {
    pub label: Rc<dyn Fn(Locale) -> String>,
    pub run: Handler,
}

/// One toast for a [`ToastHost`].
#[derive(Clone)]
pub struct ToastSpec {
    pub severity: Severity,
    pub words: Rc<dyn Fn(Locale) -> ToastWords>,
    pub action: Option<ToastAction>,
    /// `None` keeps it until it is closed or its owner takes it away.
    pub duration: Option<Duration>,
    pub closable: bool,
}

impl ToastSpec {
    /// A toast that goes away after [`TOAST_DURATION`].
    pub fn timed(severity: Severity, words: impl Fn(Locale) -> ToastWords + 'static) -> Self {
        Self {
            severity,
            words: Rc::new(words),
            action: None,
            duration: Some(TOAST_DURATION),
            closable: false,
        }
    }

    /// A toast that stays until it is closed or its owner takes it away.
    pub fn lasting(severity: Severity, words: impl Fn(Locale) -> ToastWords + 'static) -> Self {
        Self {
            severity,
            words: Rc::new(words),
            action: None,
            duration: None,
            closable: true,
        }
    }

    pub fn action(
        mut self,
        label: impl Fn(Locale) -> String + 'static,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some(ToastAction {
            label: Rc::new(label),
            run: Rc::new(run),
        });
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }
}

/// A window's toasts. Keep one per window ([`new_host`]), push to it from
/// anywhere in the window, and draw it with [`toast_layer`].
#[derive(Default)]
pub struct ToastHost {
    queue: ToastQueue<ToastSpec>,
    _tick: Option<Task<()>>,
}

impl ToastHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, spec: ToastSpec, cx: &mut Context<Self>) -> ToastId {
        let duration = spec.duration;
        let id = self.queue.push(spec, duration, Instant::now());
        self.schedule(cx);
        cx.notify();
        id
    }

    pub fn dismiss(&mut self, id: ToastId, cx: &mut Context<Self>) {
        if self.queue.dismiss(id, Instant::now()) {
            self.schedule(cx);
            cx.notify();
        }
    }

    pub fn contains(&self, id: ToastId) -> bool {
        self.queue.contains(id)
    }

    pub fn queue(&self) -> &ToastQueue<ToastSpec> {
        &self.queue
    }

    fn hover(&mut self, id: ToastId, hovered: bool, cx: &mut Context<Self>) {
        self.queue.hover(id, hovered, Instant::now());
        self.schedule(cx);
    }

    /// Sleeps until the next shown toast's time is up, hides it, and goes
    /// again; nothing runs while no toast is counting down.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        let Some(first) = self.queue.next_deadline(Instant::now()) else {
            self._tick = None;
            return;
        };
        self._tick = Some(cx.spawn(async move |this, cx| {
            let mut wait = first;
            loop {
                cx.background_executor()
                    .timer(wait.max(Duration::from_millis(50)))
                    .await;
                let next = this.update(cx, |host, cx| {
                    let now = Instant::now();
                    if host.queue.expire(now) {
                        cx.notify();
                    }
                    host.queue.next_deadline(now)
                });
                match next {
                    Ok(Some(next)) => wait = next,
                    _ => return,
                }
            }
        }));
    }
}

/// A window's host, and the subscription that redraws the window when a toast
/// comes or goes. Keep both.
pub fn new_host<V: 'static>(cx: &mut Context<V>) -> (Entity<ToastHost>, gpui::Subscription) {
    let host = cx.new(|_| ToastHost::new());
    let subscription = cx.observe(&host, |_, _, cx| cx.notify());
    (host, subscription)
}

/// Draws a window's toasts: a column at the bottom right, above dialogs. Add
/// it as the last child of the window's root element.
pub fn toast_layer(host: &Entity<ToastHost>, locale: Locale, cx: &App) -> Option<AnyElement> {
    let shown = host.read(cx).queue.shown();
    if shown.is_empty() {
        return None;
    }
    let weak = host.downgrade();
    let cards: Vec<AnyElement> = shown
        .iter()
        .map(|toast| {
            let id = toast.id;
            let spec = &toast.item;
            let words = (spec.words)(locale);
            let mut card = Toast::new(
                ElementId::NamedInteger("better-ui-toast".into(), id),
                spec.severity,
                words.title,
            )
            .locale(locale);
            if let Some(message) = words.message {
                card = card.message(message);
            }
            if let Some(toast_action) = &spec.action {
                let run = toast_action.run.clone();
                let weak = weak.clone();
                card = card.action((toast_action.label)(locale), move |window, cx| {
                    run(window, cx);
                    let _ = weak.update(cx, |host, cx| host.dismiss(id, cx));
                });
            }
            if spec.closable {
                let weak = weak.clone();
                card = card.on_close(move |_, cx| {
                    let _ = weak.update(cx, |host, cx| host.dismiss(id, cx));
                });
            }
            if toast.timer.is_some() {
                let weak = weak.clone();
                card = card.on_hover(move |hovered, _, cx| {
                    let hovered = *hovered;
                    let _ = weak.update(cx, |host, cx| host.hover(id, hovered, cx));
                });
            }
            card.into_any_element()
        })
        .collect();
    Some(
        deferred(
            div()
                .absolute()
                .right(px(TOAST_MARGIN))
                .bottom(px(TOAST_MARGIN))
                .flex()
                .flex_col()
                .items_end()
                .gap(spacing::SM)
                .children(cards),
        )
        .with_priority(TOAST_LAYER_PRIORITY)
        .into_any_element(),
    )
}

/// Told whether the pointer is on the toast.
type HoverHandler = Box<dyn Fn(&bool, &mut Window, &mut App)>;

/// The toast card.
#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    severity: Severity,
    title: SharedString,
    message: Option<SharedString>,
    action: Option<(SharedString, Handler)>,
    on_close: Option<Handler>,
    on_hover: Option<HoverHandler>,
    locale: Locale,
}

impl Toast {
    pub fn new(
        id: impl Into<ElementId>,
        severity: Severity,
        title: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            severity,
            title: title.into(),
            message: None,
            action: None,
            on_close: None,
            on_hover: None,
            locale: Locale::English,
        }
    }

    pub fn message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// The one action a toast may offer, drawn as a standard button.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }

    /// Shows the close button.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    /// Told when the pointer comes onto the toast and leaves it, which is what
    /// [`ToastTimer::hover`] wants.
    pub fn on_hover(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_hover = Some(Box::new(handler));
        self
    }

    /// The language of the close button's tooltip.
    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }
}

impl RenderOnce for Toast {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let close = self.on_close.map(|handler| {
            div().flex_none().child(
                Button::icon_button(
                    super::child_id(&self.id, "close"),
                    IconName::Close,
                    action(self.locale, Action::Dismiss),
                )
                .on_click(move |window, cx| handler(window, cx)),
            )
        });
        let action_button = self.action.map(|(label, handler)| {
            div().pt(spacing::SM).child(
                Button::standard(super::child_id(&self.id, "action"), label)
                    .on_click(move |window, cx| handler(window, cx)),
            )
        });
        let right = if close.is_some() {
            spacing::SM
        } else {
            spacing::LG
        };
        let mut card = panel_frame(div().id(self.id), p)
            .flex_row()
            .items_start()
            .gap(px(12.0))
            .max_w(px(TOAST_MAX_WIDTH))
            .pl(spacing::LG)
            .pr(right)
            .py(spacing::MD)
            .child(div().pt(px(2.0)).child(severity_icon(self.severity, p)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        text(div(), BODY_STRONG)
                            .text_color(p.text_primary)
                            .whitespace_normal()
                            .child(self.title),
                    )
                    .children(self.message.map(|message| {
                        text(div(), BODY)
                            .text_color(p.text_secondary)
                            .whitespace_normal()
                            .child(message)
                    }))
                    .children(action_button),
            )
            .children(close);
        if let Some(handler) = self.on_hover {
            card = card.on_hover(move |hovered, window, cx| handler(hovered, window, cx));
        }
        card
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SHOWN, TOAST_DURATION, TOAST_MIN_REMAINING, ToastQueue, ToastTimer};
    use std::time::Duration;
    use std::time::Instant;

    #[test]
    fn a_toast_goes_away_after_its_duration() {
        let start = Instant::now();
        let timer = ToastTimer::new(TOAST_DURATION, start);
        assert!(!timer.expired(start));
        assert!(!timer.expired(start + TOAST_DURATION - Duration::from_millis(1)));
        assert!(timer.expired(start + TOAST_DURATION));
    }

    #[test]
    fn the_pointer_on_a_toast_stops_its_clock() {
        let start = Instant::now();
        let mut timer = ToastTimer::new(TOAST_DURATION, start);
        timer.hover(true, start + Duration::from_secs(2));
        // Ten minutes under the pointer is still not expired.
        let later = start + Duration::from_secs(600);
        assert!(!timer.expired(later));
        assert_eq!(timer.time_left(later), Duration::from_secs(4));
        timer.hover(false, later);
        assert!(!timer.expired(later + Duration::from_secs(3)));
        assert!(timer.expired(later + Duration::from_secs(4)));
    }

    #[test]
    fn leaving_a_toast_leaves_at_least_a_second() {
        let start = Instant::now();
        let mut timer = ToastTimer::new(TOAST_DURATION, start);
        timer.hover(true, start + Duration::from_millis(5_900));
        let left = start + Duration::from_secs(7);
        timer.hover(false, left);
        assert!(!timer.expired(left + TOAST_MIN_REMAINING - Duration::from_millis(1)));
        assert!(timer.expired(left + TOAST_MIN_REMAINING));
    }

    #[test]
    fn hovering_twice_changes_nothing() {
        let start = Instant::now();
        let mut timer = ToastTimer::new(TOAST_DURATION, start);
        timer.hover(true, start + Duration::from_secs(1));
        let once = timer;
        timer.hover(true, start + Duration::from_secs(3));
        assert_eq!(timer, once);
    }

    #[test]
    fn toasts_beyond_the_shown_ones_wait_and_their_clock_starts_when_shown() {
        let start = Instant::now();
        let mut queue = ToastQueue::default();
        let ids: Vec<_> = (0..MAX_SHOWN + 1)
            .map(|n| queue.push(n, Some(TOAST_DURATION), start))
            .collect();
        assert_eq!(queue.shown().len(), MAX_SHOWN);
        let last = *ids.last().unwrap();
        assert!(queue.contains(last));
        assert!(queue.shown().iter().all(|toast| toast.id != last));

        // The first ones expire together; the last then starts its clock.
        let later = start + TOAST_DURATION;
        assert!(queue.expire(later));
        assert_eq!(queue.shown().len(), 1);
        assert_eq!(queue.shown()[0].id, last);
        assert_eq!(queue.next_deadline(later), Some(TOAST_DURATION));
        assert!(queue.expire(later + TOAST_DURATION));
        assert!(queue.is_empty());
    }

    #[test]
    fn a_lasting_toast_stays_until_dismissed_and_a_dismissal_lets_the_next_in() {
        let start = Instant::now();
        let mut queue = ToastQueue::default();
        let lasting = queue.push("waiting", None, start);
        for _ in 1..MAX_SHOWN {
            queue.push("other", None, start);
        }
        let queued = queue.push("queued", None, start);
        assert_eq!(queue.next_deadline(start), None, "nothing is counting");
        assert!(!queue.expire(start + Duration::from_secs(3600)));
        assert!(!queue.shown().iter().any(|toast| toast.id == queued));
        assert!(queue.dismiss(lasting, start));
        assert!(queue.shown().iter().any(|toast| toast.id == queued));
        assert!(!queue.dismiss(lasting, start), "already gone");
    }

    #[test]
    fn a_timed_toast_is_not_starved_by_toasts_that_stay() {
        let start = Instant::now();
        let mut queue = ToastQueue::default();
        for _ in 0..MAX_SHOWN {
            queue.push("stays", None, start);
        }
        let lasting_behind = queue.push("stays too", None, start);
        let timed = queue.push("timed", Some(TOAST_DURATION), start);
        assert!(
            queue.shown().iter().any(|toast| toast.id == timed),
            "shown above the limit"
        );
        assert!(!queue.shown().iter().any(|toast| toast.id == lasting_behind));
        assert_eq!(queue.shown().len(), MAX_SHOWN + 1);
        assert!(queue.expire(start + TOAST_DURATION));
        assert_eq!(queue.shown().len(), MAX_SHOWN);
    }

    #[test]
    fn hovering_one_toast_holds_only_that_one() {
        let start = Instant::now();
        let mut queue = ToastQueue::default();
        let held = queue.push(1, Some(TOAST_DURATION), start);
        let free = queue.push(2, Some(TOAST_DURATION), start);
        queue.hover(held, true, start + Duration::from_secs(1));
        assert!(queue.expire(start + TOAST_DURATION));
        assert!(queue.contains(held));
        assert!(!queue.contains(free));
        assert_eq!(queue.next_deadline(start + TOAST_DURATION), None);
        queue.hover(held, false, start + Duration::from_secs(60));
        assert_eq!(
            queue.next_deadline(start + Duration::from_secs(60)),
            Some(Duration::from_secs(5))
        );
    }
}
