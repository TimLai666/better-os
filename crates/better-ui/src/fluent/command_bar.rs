//! The command bar (WinUI `CommandBar` with `DefaultLabelPosition="Right"`,
//! as File Explorer draws it) and its buttons.
//!
//! `CommandBar_themeresources.xaml` and `AppBarButton_themeresources.xaml`:
//! a 48px bar (`AppBarThemeCompactHeight`) with no fill; buttons inset 6px
//! from its top and bottom and 2px from each other (`AppBarButtonInnerBorderMargin`),
//! transparent at rest, `SubtleFillColorSecondary` under the pointer,
//! `SubtleFillColorTertiary` pressed with secondary text; a checked toggle
//! command is accent (`AppBarToggleButtonBackgroundChecked`); a separator is a
//! 1px divider; commands that do not fit, or that are secondary, go behind a
//! trailing `…` that opens a menu flyout.
//!
//! Keyboard: Left and Right move focus between the commands, as WinUI's
//! command bar does; each command invokes on Enter or Space.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement, IntoElement, ParentElement, Pixels, RenderOnce,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName};

use super::Handler;
use super::button::{self, ButtonKind};
use super::menu::{MenuEntry, MenuFlyout, MenuItem, MenuOpen, open_menu};
use super::words::{Action, Locale, action};
use super::{
    BODY, Interaction, Key, ShownState, child_id, keyed_focus, paint, palette, radius, resolve,
    shows_focus, text, with_focus_ring,
};

/// How many commands of these widths fit in `available`, leaving room for
/// the `…` button when any do not.
pub fn fit_count(widths: &[Pixels], available: Pixels, overflow_width: Pixels) -> usize {
    let total: Pixels = widths.iter().copied().fold(px(0.0), |a, b| a + b);
    if total <= available {
        return widths.len();
    }
    let mut used = overflow_width;
    widths
        .iter()
        .take_while(|width| {
            used += **width;
            used <= available
        })
        .count()
}

/// One command.
#[derive(Clone)]
pub struct Command {
    pub label: SharedString,
    pub icon: IconName,
    /// `Some` for a toggle command (`AppBarToggleButton`).
    pub checked: Option<bool>,
    /// Draw only the icon, with the label as the tooltip.
    pub icon_only: bool,
    pub shortcut: Option<SharedString>,
    pub disabled: bool,
    pub shown: ShownState,
    pub on_click: Option<Handler>,
    /// Draws a trailing chevron: the command opens a menu.
    chevron: bool,
    /// An SVG the window serves itself, drawn instead of `icon` in the bar
    /// (the overflow menu, which takes only kit icons, keeps `icon`).
    pub icon_path: Option<SharedString>,
    /// A short count on the icon's corner (WinUI's `InfoBadge`), such as how
    /// many jobs are running.
    pub badge: Option<SharedString>,
}

impl Command {
    pub fn new(label: impl Into<SharedString>, icon: IconName) -> Self {
        Self {
            label: label.into(),
            icon,
            checked: None,
            icon_only: false,
            shortcut: None,
            disabled: false,
            shown: ShownState::Live,
            on_click: None,
            chevron: false,
            icon_path: None,
            badge: None,
        }
    }

    /// Draws the window's own SVG at `path` in the bar instead of the kit
    /// icon, for a glyph the kit does not have.
    pub fn icon_path(mut self, path: impl Into<SharedString>) -> Self {
        self.icon_path = Some(path.into());
        self
    }

    /// Puts a short count on the icon's corner; `None` takes it off.
    pub fn badge(mut self, badge: Option<impl Into<SharedString>>) -> Self {
        self.badge = badge.map(Into::into);
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn shown(mut self, shown: ShownState) -> Self {
        self.shown = shown;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn menu_entry(&self) -> MenuEntry {
        let mut item = MenuItem::new(self.label.clone())
            .icon(self.icon.clone())
            .disabled(self.disabled);
        if let Some(path) = self.icon_path.clone() {
            item = item.icon_path(path);
        }
        if let Some(shortcut) = self.shortcut.clone() {
            item = item.shortcut(shortcut);
        }
        if let Some(checked) = self.checked {
            item = item.checked(checked);
        }
        if let Some(handler) = self.on_click.clone() {
            item = item.on_select(move |window, cx| handler(window, cx));
        }
        item.entry()
    }
}

/// A line in the bar: a command, a command that opens a menu, or a
/// separator.
#[derive(Clone)]
pub enum BarEntry {
    Command(Command),
    /// A drop-down command, as File Explorer's Sort and View are: a button
    /// with a chevron that opens a menu flyout, and a submenu once it has
    /// moved into the `…` menu.
    Menu(Command, Vec<MenuEntry>),
    Separator,
}

/// Which primary entries stay in the bar and which move into the `…` menu,
/// by index. A separator stays only while a command after it stays, so the
/// bar never ends on a divider.
pub fn split_primary(kinds: &[bool], max_primary: Option<usize>) -> (Vec<usize>, Vec<usize>) {
    // `kinds[i]` is true for a command (plain or menu), false for a separator.
    let mut shown = Vec::new();
    let mut overflow = Vec::new();
    let mut commands_seen = 0;
    for (index, is_command) in kinds.iter().copied().enumerate() {
        if is_command {
            commands_seen += 1;
            if max_primary.is_some_and(|max| commands_seen > max) {
                overflow.push(index);
            } else {
                shown.push(index);
            }
        } else if max_primary.is_none_or(|max| commands_seen < max) {
            shown.push(index);
        }
    }
    while let Some(&last) = shown.last() {
        if kinds[last] {
            break;
        }
        shown.pop();
    }
    (shown, overflow)
}

/// A command bar button that opens a menu (`AppBarButton` with a `Flyout`).
#[derive(IntoElement)]
pub struct AppBarMenuButton {
    id: ElementId,
    command: Command,
    entries: Vec<MenuEntry>,
}

impl AppBarMenuButton {
    pub fn new(id: impl Into<ElementId>, command: Command, entries: Vec<MenuEntry>) -> Self {
        Self {
            id: id.into(),
            command,
            entries,
        }
    }
}

impl RenderOnce for AppBarMenuButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let menu = MenuOpen::keyed(child_id(&self.id, "menu"), window, cx);
        let open = menu.read(cx).is_open();
        let button_id = child_id(&self.id, "button");
        let focus = keyed_focus(button_id.clone(), window, cx);
        let open_state = menu.clone();
        let mut command = self.command;
        command.chevron = true;
        let command = command.on_click(move |window, cx| open_menu(&open_state, None, window, cx));
        let mut holder = div()
            .relative()
            .child(AppBarButton::new(button_id, command));
        if open {
            holder = holder.child(
                MenuFlyout::new(child_id(&self.id, "flyout"), self.entries)
                    .state(menu)
                    .return_focus(focus),
            );
        }
        holder
    }
}

/// A command bar button (`AppBarButton`), usable outside a bar.
#[derive(IntoElement)]
pub struct AppBarButton {
    id: ElementId,
    command: Command,
}

impl AppBarButton {
    pub fn new(id: impl Into<ElementId>, command: Command) -> Self {
        Self {
            id: id.into(),
            command,
        }
    }
}

impl RenderOnce for AppBarButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let command = self.command;
        let forced = resolve(command.disabled, command.shown);
        let disabled = forced == Some(Interaction::Disabled);
        let focus = keyed_focus(self.id.clone(), window, cx);
        let ring = !disabled && shows_focus(&focus, command.shown, window);
        let colors = button::colors(ButtonKind::Subtle, command.checked.unwrap_or(false), p);
        let mut element = div()
            .id(self.id.clone())
            .h(px(36.0))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .when(command.icon_only, |b| b.w(px(40.0)))
            .when(!command.icon_only, |b| b.px(px(12.0)));
        element = text(element, BODY);
        if !disabled {
            element = element.track_focus(&focus.tab_index(0).tab_stop(command.shown.is_live()));
        }
        let icon = match &command.icon_path {
            Some(path) => Icon::empty().path(path.clone()),
            None => Icon::new(command.icon.clone()),
        }
        .size(super::size::ICON);
        // On a checked (accent) button the badge turns over, so it never
        // sits accent on accent.
        let (badge_fill, badge_text) = if command.checked.unwrap_or(false) {
            (p.text_on_accent, p.accent)
        } else {
            (p.accent, p.text_on_accent)
        };
        // Only a command with a badge gets the wrapper the badge hangs on,
        // so every other command's icon is laid out exactly as before.
        let icon = match command.badge.clone() {
            None => icon.into_any_element(),
            Some(badge) => div()
                .relative()
                .child(icon)
                .child(
                    text(div(), super::CAPTION)
                        .absolute()
                        .top(px(-10.0))
                        .left(px(11.0))
                        .min_w(px(16.0))
                        .h(px(16.0))
                        .px(px(4.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius::SM)
                        .bg(badge_fill)
                        .text_color(badge_text)
                        .child(badge),
                )
                .into_any_element(),
        };
        let mut element = paint(element, colors, forced, radius::SM, p.is_dark())
            .child(icon)
            .when(!command.icon_only, |b| {
                b.child(div().whitespace_nowrap().child(command.label.clone()))
            })
            .when(command.chevron, |b| {
                b.child(Icon::new(IconName::ChevronDown).size(px(12.0)))
            });
        if command.icon_only {
            element = gpui::StatefulInteractiveElement::tooltip(
                element,
                super::dialog::tooltip_builder(command.label.clone()),
            );
        }
        if !disabled && let Some(handler) = command.on_click {
            let on_key = handler.clone();
            element =
                gpui::StatefulInteractiveElement::on_click(element, move |event, window, cx| {
                    if event.is_keyboard() {
                        return;
                    }
                    handler(window, cx)
                })
                .on_key_down(move |event, window, cx| {
                    if Key::from_keystroke(&event.keystroke).is_some_and(Key::invokes) {
                        cx.stop_propagation();
                        on_key(window, cx);
                    }
                });
        }
        with_focus_ring(element, ring, radius::SM, p)
    }
}

/// `CommandBar`.
#[derive(IntoElement)]
pub struct CommandBar {
    id: ElementId,
    primary: Vec<BarEntry>,
    secondary: Vec<MenuEntry>,
    max_primary: Option<usize>,
    locale: Locale,
    /// Draw the overflow menu open in the page, for the gallery.
    show_overflow: bool,
}

impl CommandBar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            primary: Vec::new(),
            secondary: Vec::new(),
            max_primary: None,
            locale: Locale::English,
            show_overflow: false,
        }
    }

    pub fn command(mut self, command: Command) -> Self {
        self.primary.push(BarEntry::Command(command));
        self
    }

    pub fn separator(mut self) -> Self {
        self.primary.push(BarEntry::Separator);
        self
    }

    /// A drop-down command: a button that opens `entries` as a menu.
    pub fn menu(mut self, command: Command, entries: Vec<MenuEntry>) -> Self {
        self.primary.push(BarEntry::Menu(command, entries));
        self
    }

    /// A command that lives only in the `…` menu.
    pub fn secondary(mut self, command: Command) -> Self {
        self.secondary.push(command.menu_entry());
        self
    }

    /// Any menu line in the `…` menu: a submenu, a toggle item, a separator.
    pub fn secondary_entry(mut self, entry: MenuEntry) -> Self {
        self.secondary.push(entry);
        self
    }

    /// Keeps at most this many primary commands in the bar; the rest move to
    /// the front of the `…` menu. A window that measures its width decides
    /// the number with [`fit_count`].
    pub fn max_primary(mut self, count: usize) -> Self {
        self.max_primary = Some(count);
        self
    }

    pub fn locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    pub fn show_overflow(mut self) -> Self {
        self.show_overflow = true;
        self
    }
}

impl RenderOnce for CommandBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let kinds: Vec<bool> = self
            .primary
            .iter()
            .map(|entry| !matches!(entry, BarEntry::Separator))
            .collect();
        let (shown_at, overflow_at) = split_primary(&kinds, self.max_primary);
        let mut shown = Vec::new();
        let mut overflow: Vec<MenuEntry> = Vec::new();
        for (index, entry) in self.primary.into_iter().enumerate() {
            if shown_at.contains(&index) {
                shown.push(entry);
            } else if overflow_at.contains(&index) {
                overflow.push(match entry {
                    BarEntry::Command(command) => command.menu_entry(),
                    BarEntry::Menu(command, entries) => MenuEntry::Submenu {
                        label: command.label.clone(),
                        icon: Some(command.icon.clone()),
                        disabled: command.disabled,
                        entries,
                    },
                    BarEntry::Separator => MenuEntry::Separator,
                });
            }
        }
        if !overflow.is_empty() && !self.secondary.is_empty() {
            overflow.push(MenuEntry::Separator);
        }
        overflow.extend(self.secondary);

        let mut bar = div()
            .id(self.id.clone())
            .h(px(48.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(4.0))
            .on_key_down(
                |event, window, cx| match Key::from_keystroke(&event.keystroke) {
                    Some(Key::Right) => {
                        cx.stop_propagation();
                        window.focus_next(cx);
                    }
                    Some(Key::Left) => {
                        cx.stop_propagation();
                        window.focus_prev(cx);
                    }
                    _ => {}
                },
            );
        for (index, entry) in shown.into_iter().enumerate() {
            bar = bar.child(match entry {
                BarEntry::Command(command) => {
                    AppBarButton::new(gpui::ElementId::named_usize("command", index), command)
                        .into_any_element()
                }
                BarEntry::Menu(command, entries) => AppBarMenuButton::new(
                    gpui::ElementId::named_usize("menu-command", index),
                    command,
                    entries,
                )
                .into_any_element(),
                BarEntry::Separator => div()
                    .w(px(1.0))
                    .h(px(20.0))
                    .mx(px(2.0))
                    .bg(p.divider)
                    .into_any_element(),
            });
        }
        if !overflow.is_empty() {
            let menu = MenuOpen::keyed(child_id(&self.id, "overflow"), window, cx);
            let open = menu.read(cx).is_open();
            let more_id = child_id(&self.id, "more");
            let focus = keyed_focus(more_id.clone(), window, cx);
            let open_state = menu.clone();
            let more = AppBarButton::new(
                more_id,
                Command::new(action(self.locale, Action::More), IconName::Ellipsis)
                    .icon_only()
                    .on_click(move |window, cx| open_menu(&open_state, None, window, cx)),
            );
            let mut holder = div().relative().child(more);
            if self.show_overflow {
                holder = holder.child(
                    div()
                        .absolute()
                        .top(gpui::relative(1.0))
                        .right_0()
                        .mt(px(4.0))
                        .child(super::menu::MenuPanel::new(
                            child_id(&self.id, "panel"),
                            overflow,
                        )),
                );
            } else if open {
                holder = holder.child(
                    MenuFlyout::new(child_id(&self.id, "flyout"), overflow)
                        .state(menu.clone())
                        .return_focus(focus),
                );
            }
            bar = bar.child(div().flex_1()).child(holder);
        }
        bar
    }
}
