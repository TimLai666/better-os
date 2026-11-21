//! The staged-changes bar: a footer pinned to the bottom of the content
//! area while something on the page is changed and not yet applied.
//!
//! WinUI has no control of this name. It is the pattern Windows 11 Settings
//! does not need (it applies at once) and a page with a verify-after-write
//! model does: a settings page whose changes are staged, then applied
//! together and read back. It is a content-area footer, never a dialog, so
//! the page under it stays usable while it is shown.
//!
//! The look is the kit's own tokens: the `LayerFillColorDefault` content
//! fill with a 1px `CardStrokeColorDefault` rule on top, 16px in from both
//! sides and 12px top and bottom, one Body sentence on the leading edge, and
//! two buttons on the trailing edge: the accent primary first, then the
//! standard secondary, the order a `ContentDialog` puts them in. When the
//! row is too narrow the sentence wraps and the buttons stay together.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    Styled, Window, div, px,
};

use super::button::Button;
use super::{BODY, Handler, palette, spacing, text};

/// How tall the bar is with one line of text: 12px above and below a 32px
/// button row. The content above it reserves this much so its last card can
/// scroll clear of the bar.
pub const BAR_HEIGHT: gpui::Pixels = px(57.0);

/// Whether the bar is shown: only while at least one change is staged. A
/// page never shows an empty bar to say there is nothing to apply.
pub fn shows(staged: usize) -> bool {
    staged > 0
}

/// Which of the two actions can be pressed. While the changes are being
/// applied neither can: applying twice, or discarding what is being
/// written, would race the write.
pub fn actions_enabled(staged: usize, busy: bool) -> (bool, bool) {
    let live = shows(staged) && !busy;
    (live, live)
}

/// `StagedChangesBar`.
#[derive(IntoElement)]
pub struct StagedChangesBar {
    id: ElementId,
    staged: usize,
    text: SharedString,
    primary: SharedString,
    secondary: SharedString,
    busy_label: Option<SharedString>,
    busy: bool,
    on_primary: Option<Handler>,
    on_secondary: Option<Handler>,
}

impl StagedChangesBar {
    /// `staged` is how many changes wait; `text` is the page's sentence for
    /// them ("{n} changes haven't been applied."), already filled in.
    pub fn new(id: impl Into<ElementId>, staged: usize, text: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            staged,
            text: text.into(),
            primary: SharedString::default(),
            secondary: SharedString::default(),
            busy_label: None,
            busy: false,
            on_primary: None,
            on_secondary: None,
        }
    }

    /// The accent action, usually Apply.
    pub fn primary(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.primary = label.into();
        self.on_primary = Some(Rc::new(handler));
        self
    }

    /// The standard action, usually Discard.
    pub fn secondary(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.secondary = label.into();
        self.on_secondary = Some(Rc::new(handler));
        self
    }

    /// The changes are being applied: both actions are off, and the primary
    /// says so when a label is given.
    pub fn busy(mut self, busy: bool, label: Option<impl Into<SharedString>>) -> Self {
        self.busy = busy;
        self.busy_label = label.map(Into::into);
        self
    }
}

impl RenderOnce for StagedChangesBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if !shows(self.staged) {
            return div().id(self.id).into_any_element();
        }
        let p = palette(cx);
        let (primary_on, secondary_on) = actions_enabled(self.staged, self.busy);
        let primary_label = match (&self.busy_label, self.busy) {
            (Some(label), true) => label.clone(),
            _ => self.primary.clone(),
        };
        let mut primary = Button::accent(super::child_id(&self.id, "primary"), primary_label)
            .disabled(!primary_on);
        if let Some(handler) = self.on_primary {
            primary = primary.on_click(move |window, cx| handler(window, cx));
        }
        let mut secondary =
            Button::standard(super::child_id(&self.id, "secondary"), self.secondary)
                .disabled(!secondary_on);
        if let Some(handler) = self.on_secondary {
            secondary = secondary.on_click(move |window, cx| handler(window, cx));
        }
        div()
            .id(self.id)
            .relative()
            .w_full()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(spacing::MD)
            .px(spacing::LG)
            .py(spacing::MD)
            // The bar meets the window's bottom corners: its fill and rule
            // are one fill that follows them (ticket 141); the rule's width
            // stays in the layout.
            .border_t_1()
            .border_color(gpui::transparent_black())
            .child(super::window_frame::corner_fill(p.layer).border(
                gpui::Edges {
                    top: px(1.0),
                    ..gpui::Edges::default()
                },
                p.card_stroke,
            ))
            .child(
                text(div().flex_1().min_w_0(), BODY)
                    .text_color(p.text_primary)
                    .whitespace_normal()
                    .child(self.text),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .gap(spacing::SM)
                    .child(primary)
                    .child(secondary),
            )
            .into_any_element()
    }
}
