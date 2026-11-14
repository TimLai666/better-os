//! `better-fluent-gallery`: every control in `better_ui::fluent`, in every
//! state, in light and dark — the way the kit is looked at and how its
//! screenshots are taken. Like the WinUI 3 Gallery, each page shows a row
//! per control: the five WinUI states drawn side by side, then a live copy to
//! use with the pointer and the keyboard.
//!
//! It is a development tool. No package ships it and no manifest names it.

use std::cell::Cell;
use std::rc::Rc;

use better_ui::command_line::{WindowCommandLine, WindowOption};
use better_ui::fluent::auto_suggest::{AutoSuggestBox, Suggestion};
use better_ui::fluent::badge::{Badge, InfoBadge};
use better_ui::fluent::breadcrumb::BreadcrumbBar;
use better_ui::fluent::button::{Button, ButtonKind, DropDownButton, SplitButton};
use better_ui::fluent::chip::Chip;
use better_ui::fluent::chrome::{ButtonLayout, TitleBar, WindowButton};
use better_ui::fluent::command_bar::{Command, CommandBar};
use better_ui::fluent::dialog::{
    ContentDialog, DefaultButton, DialogButtons, DialogResult, Flyout, tooltip_inline,
};
use better_ui::fluent::hyperlink::HyperlinkButton;
use better_ui::fluent::info_bar::{ErrorBar, ErrorReport, InfoBar, Severity};
use better_ui::fluent::lanes::{LaneCell, LaneLine, LineKind, NodeStyle, lane_hue};
use better_ui::fluent::list::ListRow;
use better_ui::fluent::list::{Column, DetailsTable, ListItem, ListView, SortState};
use better_ui::fluent::menu::{
    ContextMenu, MenuEntry, MenuItem, MenuOpen, MenuPanel, new_state, open_menu,
};
use better_ui::fluent::navigation::{NavEntry, NavItem, NavigationView};
use better_ui::fluent::progress::{Progress, ProgressBar, ProgressRing, ProgressStatus};
use better_ui::fluent::selection::{ComboBox, ComboItem, Slider, SliderModel};
use better_ui::fluent::settings_card::{DeviceBanner, SettingsCard, SettingsExpander, SettingsRow};
use better_ui::fluent::tabs::{SelectorBar, SelectorItem, Tab, TabView, after_close};
use better_ui::fluent::text_box::{NumberBox, NumberModel, TextBox};
use better_ui::fluent::theme::Appearance;
use better_ui::fluent::toggle::{CheckBox, CheckState, RadioButtons, RadioOption, ToggleSwitch};
use better_ui::fluent::words::Locale;
use better_ui::fluent::{
    BODY, BODY_STRONG, CAPTION, Palette, SUBTITLE, ShownState, TITLE, label, palette, radius,
    spacing,
};
use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Point, Render, SharedString, StatefulInteractiveElement,
    Styled, Window, WindowBounds, div, prelude::FluentBuilder as _, px, size,
};
use gpui_component::input::InputState;
use gpui_component::{Icon, IconName};

const HELP: &str = "\
Usage: better-fluent-gallery [--light | --dark] [--zh-TW] [--page NAME] [--compact] [--dialog]

Shows every better_ui::fluent control in every state.

  --light, --dark   Start in that appearance (the desktop's is the default).
  --zh-TW           Label the controls in Taiwan Mandarin.
  --page NAME       Open a page: buttons, text, selection, status, dialogs,
                    collections, navigation, settings, window.
  --compact         Start with the navigation pane compact.
  --dialog          Open the live content dialog on start.";

const COMMAND_LINE: WindowCommandLine = WindowCommandLine {
    program: "better-fluent-gallery",
    version: env!("CARGO_PKG_VERSION"),
    help: HELP,
    options: &[
        WindowOption::Flag("--light"),
        WindowOption::Flag("--dark"),
        WindowOption::Flag("--zh-TW"),
        WindowOption::Flag("--compact"),
        WindowOption::Flag("--dialog"),
        WindowOption::Valued("--page"),
    ],
    refusal: |argument| format!("better-fluent-gallery: unknown argument {argument}; see --help"),
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Page {
    Buttons,
    Text,
    Selection,
    Status,
    Dialogs,
    Collections,
    Navigation,
    Settings,
    Window,
}

const PAGES: [Page; 9] = [
    Page::Buttons,
    Page::Text,
    Page::Selection,
    Page::Status,
    Page::Dialogs,
    Page::Collections,
    Page::Navigation,
    Page::Settings,
    Page::Window,
];

impl Page {
    fn slug(self) -> &'static str {
        match self {
            Page::Buttons => "buttons",
            Page::Text => "text",
            Page::Selection => "selection",
            Page::Status => "status",
            Page::Dialogs => "dialogs",
            Page::Collections => "collections",
            Page::Navigation => "navigation",
            Page::Settings => "settings",
            Page::Window => "window",
        }
    }

    fn title(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Page::Buttons, false) => "Buttons and toggles",
            (Page::Buttons, true) => "按鈕與切換",
            (Page::Text, false) => "Text input",
            (Page::Text, true) => "文字輸入",
            (Page::Selection, false) => "Selection",
            (Page::Selection, true) => "選取",
            (Page::Status, false) => "Status and progress",
            (Page::Status, true) => "狀態與進度",
            (Page::Dialogs, false) => "Dialogs and flyouts",
            (Page::Dialogs, true) => "對話方塊與飛出視窗",
            (Page::Collections, false) => "Lists and tables",
            (Page::Collections, true) => "清單與表格",
            (Page::Navigation, false) => "Navigation",
            (Page::Navigation, true) => "導覽",
            (Page::Settings, false) => "Settings cards",
            (Page::Settings, true) => "設定卡片",
            (Page::Window, false) => "Window chrome",
            (Page::Window, true) => "視窗外框",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Page::Buttons => IconName::CircleCheck,
            Page::Text => IconName::ALargeSmall,
            Page::Selection => IconName::ChevronsUpDown,
            Page::Status => IconName::Loader,
            Page::Dialogs => IconName::Inbox,
            Page::Collections => IconName::LayoutDashboard,
            Page::Navigation => IconName::Map,
            Page::Settings => IconName::Settings,
            Page::Window => IconName::Frame,
        }
    }
}

struct Options {
    appearance: Appearance,
    zh: bool,
    page: Page,
    compact: bool,
    dialog: bool,
}

fn options(arguments: &[String]) -> Options {
    let mut options = Options {
        appearance: Appearance::default(),
        zh: false,
        page: Page::Buttons,
        compact: false,
        dialog: false,
    };
    let mut iter = arguments.iter();
    while let Some(argument) = iter.next() {
        match argument.as_str() {
            "--light" => options.appearance = Appearance::Light,
            "--dark" => options.appearance = Appearance::Dark,
            "--zh-TW" => options.zh = true,
            "--compact" => options.compact = true,
            "--dialog" => options.dialog = true,
            "--page" => {
                if let Some(name) = iter.next() {
                    options.page = page_named(name);
                }
            }
            other => {
                if let Some(name) = other.strip_prefix("--page=") {
                    options.page = page_named(name);
                }
            }
        }
    }
    options
}

/// Whether the gallery is drawn dark right now.
fn shows_dark(cx: &App) -> bool {
    gpui_component::Theme::global(cx).mode.is_dark()
}

fn page_named(name: &str) -> Page {
    PAGES
        .into_iter()
        .find(|page| page.slug() == name)
        .unwrap_or(Page::Buttons)
}

const CRUMBS: [&str; 5] = ["Home", "Documents", "Better OS", "Design", "Screenshots"];

struct Gallery {
    zh: bool,
    page: Page,
    compact: bool,
    toggled: bool,
    check: CheckState,
    three_state: CheckState,
    radio: Option<usize>,
    switch_on: bool,
    combo: Option<usize>,
    slider: f64,
    number: f64,
    list: Option<usize>,
    table_sort: Option<SortState>,
    table_selected: Option<usize>,
    tabs: Vec<&'static str>,
    tab: Option<usize>,
    selector: Option<usize>,
    crumbs: Vec<&'static str>,
    expander: bool,
    dialog_open: bool,
    flyout_open: bool,
    dialog_result: Option<DialogResult>,
    info_open: [bool; 4],
    demo_nav: Option<usize>,
    demo_compact: bool,
    /// The search pane demo: its selection, compact state, and whether it
    /// floats over its content as a narrow window's does.
    search_nav: Option<usize>,
    search_compact: bool,
    search_overlay: bool,
    suggest_input: Entity<InputState>,
    pane_search_input: Entity<InputState>,
    context_menu: Entity<MenuOpen>,
    context_at: Point<Pixels>,
    last_action: SharedString,
    text_input: Entity<InputState>,
    focused_input: Entity<InputState>,
    password_input: Entity<InputState>,
    search_input: Entity<InputState>,
    number_input: Entity<InputState>,
    rest_input: Entity<InputState>,
    disabled_input: Entity<InputState>,
}

fn input(
    placeholder: &'static str,
    value: &'static str,
    window: &mut Window,
    cx: &mut Context<Gallery>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

impl Gallery {
    fn new(options: Options, window: &mut Window, cx: &mut Context<Self>) -> Self {
        options.appearance.apply_and_follow(window, cx);
        let zh = options.zh;
        let pick = |en: &'static str, zh_text: &'static str| if zh { zh_text } else { en };
        Self {
            zh,
            page: options.page,
            compact: options.compact,
            toggled: true,
            check: CheckState::Checked,
            three_state: CheckState::Indeterminate,
            radio: Some(1),
            switch_on: true,
            combo: Some(1),
            slider: 40.0,
            number: 12.0,
            list: Some(1),
            table_sort: Some(SortState {
                column: 0,
                ascending: true,
            }),
            table_selected: Some(2),
            tabs: vec!["README.md", "main.rs", "Cargo.toml"],
            tab: Some(1),
            selector: Some(0),
            crumbs: CRUMBS.to_vec(),
            expander: true,
            dialog_open: options.dialog,
            flyout_open: false,
            dialog_result: None,
            info_open: [true; 4],
            demo_nav: Some(1),
            demo_compact: false,
            search_nav: Some(0),
            search_compact: false,
            search_overlay: true,
            suggest_input: input(pick("Find a setting", "尋找設定"), "", window, cx),
            pane_search_input: input(pick("Find a setting", "尋找設定"), "", window, cx),
            context_menu: new_state(cx),
            context_at: Point::default(),
            last_action: "".into(),
            text_input: input(pick("Type a name", "輸入名稱"), "", window, cx),
            focused_input: input("", pick("Editing", "正在編輯"), window, cx),
            password_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .default_value("hunter2")
            }),
            search_input: input(pick("Search", "搜尋"), "", window, cx),
            number_input: input("", "12", window, cx),
            rest_input: input(pick("Placeholder", "佔位文字"), "", window, cx),
            disabled_input: input("", pick("Read only", "無法編輯"), window, cx),
        }
    }

    fn t(&self, en: &'static str, zh: &'static str) -> &'static str {
        if self.zh { zh } else { en }
    }

    fn locale(&self) -> Locale {
        if self.zh {
            Locale::TraditionalChinese
        } else {
            Locale::English
        }
    }

    /// A handler for a control that updates the gallery and repaints.
    fn set_appearance(&mut self, appearance: Appearance, window: &mut Window, cx: &mut App) {
        appearance.apply_and_follow(window, cx);
    }

    fn act(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |window, cx| {
            let _ = weak.update(cx, |this, cx| {
                f(this, window, cx);
                cx.notify();
            });
        }
    }

    fn act_with<T: 'static>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(T, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |value, window, cx| {
            let _ = weak.update(cx, |this, cx| {
                f(this, value, window, cx);
                cx.notify();
            });
        }
    }

    fn note(cx: &Context<Self>, what: &'static str) -> impl Fn(&mut Window, &mut App) + 'static {
        Self::act(cx, move |this, _, _| this.last_action = what.into())
    }
}

/// A section: a Subtitle heading and its rows on a card.
fn section(title: &str, p: &Palette, rows: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(spacing::SM)
        .child(label(title.to_string(), SUBTITLE, p.text_primary))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(spacing::MD)
                .p(spacing::LG)
                .rounded(radius::SM)
                .border_1()
                .border_color(p.card_stroke)
                .bg(p.card)
                .children(rows),
        )
        .into_any_element()
}

const COLUMN: f32 = 150.0;

/// The column names over a state matrix.
fn state_header(p: &Palette, live: &str) -> AnyElement {
    let mut row = div()
        .flex()
        .items_center()
        .gap(spacing::LG)
        .child(div().w(px(COLUMN)).flex_none());
    for state in ShownState::ALL {
        row = row.child(div().w(px(COLUMN)).flex_none().child(label(
            state.name(),
            CAPTION,
            p.text_secondary,
        )));
    }
    row.child(label(live.to_string(), CAPTION, p.text_secondary))
        .into_any_element()
}

/// One control in its five states, then live.
fn matrix(
    name: &str,
    p: &Palette,
    draw: impl Fn(ShownState) -> AnyElement,
    live: AnyElement,
) -> AnyElement {
    let mut row = div()
        .flex()
        .items_center()
        .gap(spacing::LG)
        .min_h(px(40.0))
        .child(div().w(px(COLUMN)).flex_none().child(label(
            name.to_string(),
            BODY_STRONG,
            p.text_primary,
        )));
    for state in ShownState::ALL {
        row = row.child(
            div()
                .w(px(COLUMN))
                .flex_none()
                .flex()
                .items_center()
                .child(draw(state)),
        );
    }
    row.child(div().flex_none().flex().items_center().child(live))
        .into_any_element()
}

/// A labelled row that is not a state matrix.
/// Five rows of a branch graph: a merge, the branch it merged, and the fork
/// both came from, as Better Git draws them.
fn graph_demo(p: &Palette) -> AnyElement {
    let l = |kind, column, lane| LaneLine { kind, column, lane };
    let rows: [(&str, u16, u32, NodeStyle, Vec<LaneLine>); 5] = [
        (
            "Merge feature/login",
            0,
            0,
            NodeStyle::Head,
            vec![l(LineKind::Out, 0, 0), l(LineKind::Out, 1, 1)],
        ),
        (
            "Remember the last user",
            1,
            1,
            NodeStyle::Commit,
            vec![
                l(LineKind::Pass, 0, 0),
                l(LineKind::In, 1, 1),
                l(LineKind::Out, 1, 1),
            ],
        ),
        (
            "Fix typo in README",
            0,
            0,
            NodeStyle::Commit,
            vec![
                l(LineKind::Pass, 1, 1),
                l(LineKind::In, 0, 0),
                l(LineKind::Out, 0, 0),
            ],
        ),
        (
            "Add login form",
            1,
            1,
            NodeStyle::Commit,
            vec![
                l(LineKind::Pass, 0, 0),
                l(LineKind::In, 1, 1),
                l(LineKind::Out, 1, 1),
            ],
        ),
        (
            "Set up the build",
            0,
            0,
            NodeStyle::Commit,
            vec![l(LineKind::In, 0, 0), l(LineKind::In, 1, 1)],
        ),
    ];
    let mut list = div().w(px(480.0)).flex().flex_col();
    for (summary, column, lane, node, lines) in rows {
        list = list.child(
            div()
                .h(px(36.0))
                .flex()
                .items_center()
                .gap(spacing::SM)
                .child(LaneCell::new(column, lane, lines, 2).node(node))
                .child(label(summary.to_string(), BODY, p.text_primary)),
        );
    }
    list.into_any_element()
}

fn line(name: &str, p: &Palette, content: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap(spacing::LG)
        .child(div().w(px(COLUMN)).flex_none().pt(px(6.0)).child(label(
            name.to_string(),
            BODY_STRONG,
            p.text_primary,
        )))
        .child(div().flex_1().min_w_0().child(content))
        .into_any_element()
}

fn sid(prefix: &'static str, state: ShownState) -> ElementId {
    ElementId::named_usize(prefix, state as usize)
}

fn caption_over(caption: &str, p: &Palette, element: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(spacing::XS)
        .child(label(caption.to_string(), CAPTION, p.text_secondary))
        .child(element)
}

impl Gallery {
    fn buttons_page(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let live = self.t("Live", "實際操作");
        let save = self.t("Save", "儲存");
        let cancel = self.t("Cancel", "取消");
        let more = self.t("More", "更多");
        let split_entries = || {
            vec![
                MenuItem::new(self.t("Save as…", "另存新檔…"))
                    .shortcut("Ctrl+Shift+S")
                    .entry(),
                MenuItem::new(self.t("Save a copy", "儲存複本")).entry(),
            ]
        };
        let mut rows = vec![state_header(p, live)];
        rows.push(matrix(
            self.t("Accent", "輔色按鈕"),
            p,
            |s| {
                Button::accent(sid("accent", s), save)
                    .shown(s)
                    .into_any_element()
            },
            Button::accent("accent-live", save)
                .on_click(Self::note(cx, "accent"))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Standard", "標準按鈕"),
            p,
            |s| {
                Button::standard(sid("standard", s), cancel)
                    .shown(s)
                    .into_any_element()
            },
            Button::standard("standard-live", cancel)
                .on_click(Self::note(cx, "standard"))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Hyperlink", "超連結按鈕"),
            p,
            |s| {
                HyperlinkButton::new(sid("link", s), self.t("Source code", "專案原始碼"))
                    .external(true)
                    .shown(s)
                    .into_any_element()
            },
            HyperlinkButton::new("link-live", self.t("Source code", "專案原始碼"))
                .external(true)
                .on_click(Self::note(cx, "hyperlink"))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Subtle", "淡化按鈕"),
            p,
            |s| {
                Button::subtle(sid("subtle", s), more)
                    .shown(s)
                    .into_any_element()
            },
            Button::subtle("subtle-live", more)
                .on_click(Self::note(cx, "subtle"))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Icon button", "圖示按鈕"),
            p,
            |s| {
                Button::icon_button(sid("icon", s), IconName::Settings, "Settings")
                    .shown(s)
                    .into_any_element()
            },
            Button::icon_button("icon-live", IconName::Settings, self.t("Settings", "設定"))
                .on_click(Self::note(cx, "icon"))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Icon and text", "圖示與文字"),
            p,
            |s| {
                Button::standard(sid("icon-text", s), self.t("Copy", "複製"))
                    .icon(IconName::Copy)
                    .shown(s)
                    .into_any_element()
            },
            Button::standard("icon-text-live", self.t("Copy", "複製"))
                .icon(IconName::Copy)
                .on_click(Self::note(cx, "copy"))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Toggle, checked", "切換按鈕（開）"),
            p,
            |s| {
                Button::toggle(sid("toggle", s), self.t("Bold", "粗體"), true)
                    .shown(s)
                    .into_any_element()
            },
            Button::toggle("toggle-live", self.t("Bold", "粗體"), self.toggled)
                .on_click(Self::act(cx, |this, _, _| this.toggled = !this.toggled))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Split button", "分割按鈕"),
            p,
            |s| {
                SplitButton::new(sid("split", s), save)
                    .entries(split_entries())
                    .shown(s)
                    .into_any_element()
            },
            SplitButton::new("split-live", save)
                .entries(split_entries())
                .on_click(Self::note(cx, "split"))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Drop-down button", "下拉按鈕"),
            p,
            |s| {
                DropDownButton::new(sid("dropdown", s), ButtonKind::Standard)
                    .label(self.t("Sort", "排序"))
                    .entries(split_entries())
                    .shown(s)
                    .into_any_element()
            },
            DropDownButton::new("dropdown-live", ButtonKind::Standard)
                .label(self.t("Sort", "排序"))
                .entries(split_entries())
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Check box", "核取方塊"),
            p,
            |s| {
                CheckBox::new(sid("check", s), CheckState::Checked)
                    .label("Wi-Fi")
                    .shown(s)
                    .into_any_element()
            },
            CheckBox::new("check-live", self.check)
                .label(self.t("Remember me", "記住我"))
                .on_change(Self::act_with(cx, |this, value, _, _| this.check = value))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Unchecked", "未核取"),
            p,
            |s| {
                CheckBox::new(sid("check-off", s), CheckState::Unchecked)
                    .label("Wi-Fi")
                    .shown(s)
                    .into_any_element()
            },
            div().into_any_element(),
        ));
        rows.push(matrix(
            self.t("Indeterminate", "部分選取"),
            p,
            |s| {
                CheckBox::new(sid("check-mixed", s), CheckState::Indeterminate)
                    .label(self.t("Select all", "全選"))
                    .shown(s)
                    .into_any_element()
            },
            CheckBox::new("check-three-live", self.three_state)
                .label(self.t("Three-state", "三態"))
                .three_state(true)
                .on_change(Self::act_with(cx, |this, value, _, _| {
                    this.three_state = value
                }))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Toggle switch", "切換開關"),
            p,
            |s| {
                ToggleSwitch::new(sid("switch", s), true)
                    .locale(self.locale())
                    .shown(s)
                    .into_any_element()
            },
            ToggleSwitch::new("switch-live", self.switch_on)
                .locale(self.locale())
                .on_change(Self::act_with(cx, |this, value, _, _| {
                    this.switch_on = value
                }))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Switch, off", "切換開關（關）"),
            p,
            |s| {
                ToggleSwitch::new(sid("switch-off", s), false)
                    .locale(self.locale())
                    .shown(s)
                    .into_any_element()
            },
            div().into_any_element(),
        ));
        rows.push(matrix(
            self.t("Radio buttons", "選項按鈕"),
            p,
            |s| {
                let options = vec![
                    RadioOption::new(self.t("Selected", "已選取")).shown(s),
                    RadioOption::new(self.t("Not selected", "未選取")).shown(s),
                ];
                RadioButtons::new(sid("radio", s), options, Some(0))
                    .shown(s)
                    .into_any_element()
            },
            RadioButtons::new(
                "radio-live",
                vec![
                    RadioOption::new(self.t("Small", "小")),
                    RadioOption::new(self.t("Medium", "中")),
                    RadioOption::new(self.t("Large", "大")),
                ],
                self.radio,
            )
            .header(self.t("Size", "大小"))
            .on_select(Self::act_with(cx, |this, index, _, _| {
                this.radio = Some(index)
            }))
            .into_any_element(),
        ));
        vec![section(
            self.t("Buttons and toggles", "按鈕與切換"),
            p,
            rows,
        )]
    }

    fn text_page(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let live = self.t("Live", "實際操作");
        let mut rows = vec![state_header(p, live)];
        let pick = |s: ShownState| match s {
            ShownState::Focused => &self.focused_input,
            ShownState::Disabled => &self.disabled_input,
            _ => &self.rest_input,
        };
        rows.push(matrix(
            self.t("Text box", "文字方塊"),
            p,
            |s| {
                TextBox::new(sid("text", s), pick(s))
                    .width(px(COLUMN))
                    .shown(s)
                    .into_any_element()
            },
            TextBox::new("text-live", &self.text_input)
                .header(self.t("Display name", "顯示名稱"))
                .description(self.t("Shown on the lock screen.", "會顯示在鎖定畫面。"))
                .width(px(260.0))
                .into_any_element(),
        ));
        let required = self.t("Enter a name.", "請輸入名稱。");
        let empty = self.text_input.read(cx).value().trim().is_empty();
        rows.push(matrix(
            self.t("With a reason", "顯示原因"),
            p,
            |s| {
                TextBox::new(sid("text-error", s), pick(s))
                    .width(px(COLUMN))
                    .error(Some(required))
                    .shown(s)
                    .into_any_element()
            },
            TextBox::new("text-error-live", &self.text_input)
                .header(self.t("Display name", "顯示名稱"))
                .description(self.t("Shown on the lock screen.", "會顯示在鎖定畫面。"))
                .error(empty.then_some(required))
                .width(px(260.0))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Password box", "密碼方塊"),
            p,
            |s| {
                TextBox::password(sid("password", s), &self.password_input)
                    .width(px(COLUMN))
                    .shown(s)
                    .into_any_element()
            },
            TextBox::password("password-live", &self.password_input)
                .header(self.t(
                    "Password (hold the eye to show it)",
                    "密碼（按住眼睛圖示以顯示）",
                ))
                .width(px(260.0))
                .into_any_element(),
        ));
        rows.push(matrix(
            self.t("Search box", "搜尋方塊"),
            p,
            |s| {
                TextBox::search(sid("search", s), pick(s))
                    .width(px(COLUMN))
                    .shown(s)
                    .into_any_element()
            },
            TextBox::search("search-live", &self.search_input)
                .width(px(260.0))
                .on_search(Self::note(cx, "search"))
                .into_any_element(),
        ));
        let model = NumberModel {
            minimum: 0.0,
            maximum: 100.0,
            small_change: 1.0,
            large_change: 10.0,
            wrap: false,
        };
        rows.push(matrix(
            self.t("Number box", "數字方塊"),
            p,
            |s| {
                NumberBox::new(
                    sid("number", s),
                    if s == ShownState::Disabled {
                        &self.disabled_input
                    } else {
                        &self.number_input
                    },
                    self.number,
                    model,
                )
                .width(px(COLUMN))
                .shown(s)
                .into_any_element()
            },
            NumberBox::new("number-live", &self.number_input, self.number, model)
                .header(self.t(
                    "Copies (0–100; Up, Down, Page Up, Page Down)",
                    "份數（0–100；上、下、Page Up、Page Down）",
                ))
                .width(px(260.0))
                .on_change(Self::act_with(cx, |this, value, _, _| this.number = value))
                .into_any_element(),
        ));
        rows.push(line(
            self.t("Value", "目前的值"),
            p,
            label(NumberModel::format(self.number), BODY, p.text_secondary),
        ));
        vec![section(self.t("Text input", "文字輸入"), p, rows)]
    }

    fn combo_items(&self) -> Vec<ComboItem> {
        vec![
            ComboItem::new(self.t("Follow the system", "跟隨系統")),
            ComboItem::new(self.t("Dark", "深色")),
            ComboItem::new(self.t("Light", "淺色")),
            ComboItem::new(self.t("High contrast (unavailable)", "高對比（無法使用）"))
                .disabled(true),
        ]
    }

    fn selection_page(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let live = self.t("Live", "實際操作");
        let mut rows = vec![state_header(p, live)];
        rows.push(matrix(
            self.t("Combo box", "下拉式方塊"),
            p,
            |s| {
                ComboBox::new(sid("combo", s), self.combo_items(), Some(1))
                    .min_width(px(140.0))
                    .shown(s)
                    .into_any_element()
            },
            ComboBox::new("combo-live", self.combo_items(), self.combo)
                .header(self.t("Appearance", "外觀"))
                .min_width(px(200.0))
                .on_select(Self::act_with(cx, |this, index, _, _| {
                    this.combo = Some(index)
                }))
                .into_any_element(),
        ));
        rows.push(line(
            self.t("Open, placeholder", "展開、提示文字"),
            p,
            div()
                .flex()
                .items_start()
                .gap(spacing::XL)
                .h(px(210.0))
                .child(
                    ComboBox::new("combo-open", self.combo_items(), Some(1))
                        .min_width(px(240.0))
                        .show_open(Some(2)),
                )
                .child(
                    ComboBox::new("combo-placeholder", self.combo_items(), None)
                        .placeholder(self.t("Pick one", "請選擇"))
                        .min_width(px(200.0)),
                ),
        ));
        let model = SliderModel {
            minimum: 0.0,
            maximum: 100.0,
            step: 1.0,
            large_change: 10.0,
        };
        rows.push(matrix(
            self.t("Slider", "滑桿"),
            p,
            |s| {
                Slider::new(sid("slider", s), 40.0, model)
                    .width(px(140.0))
                    .shown(s)
                    .into_any_element()
            },
            Slider::new("slider-live", self.slider, model)
                .header(format!("{} {}", self.t("Volume", "音量"), self.slider))
                .width(px(240.0))
                .on_change(Self::act_with(cx, |this, value, _, _| this.slider = value))
                .into_any_element(),
        ));
        vec![section(self.t("Selection", "選取"), p, rows)]
    }

    fn status_page(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let bars = div()
            .flex()
            .flex_col()
            .gap(spacing::LG)
            .w(px(480.0))
            .child(caption_over(
                self.t("Determinate, 60%", "確定進度 60%"),
                p,
                ProgressBar::new("bar-60", Progress::Determinate(0.6)),
            ))
            .child(caption_over(
                self.t("Indeterminate", "不確定進度"),
                p,
                ProgressBar::new("bar-ind", Progress::Indeterminate),
            ))
            .child(caption_over(
                self.t("Paused", "已暫停"),
                p,
                ProgressBar::new("bar-paused", Progress::Determinate(0.45))
                    .status(ProgressStatus::Paused),
            ))
            .child(caption_over(
                self.t("Error", "錯誤"),
                p,
                ProgressBar::new("bar-error", Progress::Determinate(0.8))
                    .status(ProgressStatus::Error),
            ));
        let rings = div()
            .flex()
            .items_center()
            .gap(spacing::XL)
            .child(ProgressRing::new("ring-ind", Progress::Indeterminate))
            .child(ProgressRing::new("ring-frozen", Progress::Indeterminate).frozen(0.3))
            .child(ProgressRing::new("ring-70", Progress::Determinate(0.7)))
            .child(ProgressRing::new("ring-small", Progress::Indeterminate).size(px(16.0)))
            .child(label(
                self.t(
                    "Indeterminate (moving, still), 70%, 16px",
                    "不確定（轉動、靜止）、70%、16px",
                ),
                CAPTION,
                p.text_secondary,
            ));
        let progress = vec![
            line(self.t("Progress bar", "進度列"), p, bars),
            line(self.t("Progress ring", "進度環"), p, rings),
        ];
        let info = [
            (
                Severity::Informational,
                self.t("Update available", "有可用的更新"),
                self.t(
                    "Better Files 0.3.0 is ready to install.",
                    "Better Files 0.3.0 已可安裝。",
                ),
            ),
            (
                Severity::Success,
                self.t("Settings applied", "已套用設定"),
                self.t("Everything works as expected.", "一切運作正常。"),
            ),
            (
                Severity::Warning,
                self.t("Running without the service", "未連線到服務"),
                self.t(
                    "Some changes will not be kept after you sign out.",
                    "部分變更在登出後不會保留。",
                ),
            ),
            (
                Severity::Error,
                self.t("Couldn't save", "無法儲存"),
                self.t(
                    "The disk is full. Free some space and try again.",
                    "磁碟已滿，請釋放空間後再試一次。",
                ),
            ),
        ];
        let mut bars = Vec::new();
        for (index, (severity, title, message)) in info.into_iter().enumerate() {
            if !self.info_open[index] {
                continue;
            }
            let mut bar = InfoBar::new(ElementId::named_usize("info", index), severity, title)
                .message(message)
                .locale(self.locale())
                .on_close(Self::act(cx, move |this, _, _| {
                    this.info_open[index] = false
                }));
            if severity == Severity::Informational {
                bar = bar.action(Button::standard("info-action", self.t("Install", "安裝")));
            }
            bars.push(bar.into_any_element());
        }
        if bars.len() < 4 {
            bars.push(
                div()
                    .flex()
                    .child(
                        Button::standard(
                            "info-reset",
                            self.t("Show the info bars again", "重新顯示資訊列"),
                        )
                        .on_click(Self::act(cx, |this, _, _| this.info_open = [true; 4])),
                    )
                    .into_any_element(),
            );
        }
        let report = ErrorReport::new(
            self.t(
                "Better Git couldn't open this folder.",
                "Better Git 無法開啟這個資料夾。",
            ),
            "git rev-parse --show-toplevel\nfatal: not a git repository (or any of the parent directories): .git\nexit status 128",
        )
        .advice(self.t(
            "Choose a folder that holds a Git repository.",
            "請選擇含有 Git 儲存庫的資料夾。",
        ));
        vec![
            section(self.t("Progress", "進度"), p, progress),
            section(
                self.t("Info bar", "資訊列"),
                p,
                vec![
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .children(bars)
                        .into_any_element(),
                ],
            ),
            section(
                self.t("Toast", "快顯通知"),
                p,
                // The notices a window shows about an update (ticket 105),
                // and a toast with an action, drawn in place rather than
                // floating over the page.
                {
                    let locale = self.locale();
                    let mut cards: Vec<AnyElement> = [
                        better_ui::restart::Notice::Updated {
                            version: env!("CARGO_PKG_VERSION").to_string(),
                        },
                        better_ui::restart::Notice::Waiting {
                            app: "Better Files",
                            blocker: better_ui::restart::Blocker::Dialog,
                        },
                        better_ui::restart::Notice::ReopenToUse,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, notice)| {
                        let (severity, words) = notice.words(locale);
                        let mut toast = better_ui::fluent::toast::Toast::new(
                            ElementId::named_usize("toast", index),
                            severity,
                            words.title,
                        )
                        .locale(locale);
                        if let Some(message) = words.message {
                            toast = toast.message(message);
                        }
                        if notice.toast().closable {
                            toast = toast.on_close(|_, _| {});
                        }
                        div().flex().child(toast).into_any_element()
                    })
                    .collect();
                    cards.push(
                        div()
                            .flex()
                            .child(
                                better_ui::fluent::toast::Toast::new(
                                    "toast-action",
                                    Severity::Success,
                                    self.t("Moved 3 items to the Trash", "已將 3 個項目移到垃圾桶"),
                                )
                                .locale(locale)
                                .action(self.t("Undo", "復原"), |_, _| {}),
                            )
                            .into_any_element(),
                    );
                    cards
                },
            ),
            section(
                self.t("Error with details", "錯誤與詳細資料"),
                p,
                vec![
                    ErrorBar::new("error-closed", report.clone())
                        .locale(self.locale())
                        .into_any_element(),
                    ErrorBar::new("error-open", report)
                        .locale(self.locale())
                        .expanded(true)
                        .into_any_element(),
                ],
            ),
        ]
    }

    fn menu_entries(&self, cx: &Context<Self>) -> Vec<MenuEntry> {
        vec![
            MenuItem::new(self.t("Cut", "剪下"))
                .shortcut("Ctrl+X")
                .on_select(Self::note(cx, "cut"))
                .entry(),
            MenuItem::new(self.t("Copy", "複製"))
                .icon(IconName::Copy)
                .shortcut("Ctrl+C")
                .on_select(Self::note(cx, "copy"))
                .entry(),
            MenuItem::new(self.t("Paste", "貼上"))
                .shortcut("Ctrl+V")
                .disabled(true)
                .entry(),
            MenuEntry::Separator,
            MenuEntry::submenu(
                self.t("Sort by", "排序方式"),
                vec![
                    MenuItem::new(self.t("Name", "名稱"))
                        .on_select(Self::note(cx, "sort by name"))
                        .entry(),
                    MenuItem::new(self.t("Date modified", "修改日期"))
                        .on_select(Self::note(cx, "sort by date"))
                        .entry(),
                    MenuItem::new(self.t("Size", "大小"))
                        .on_select(Self::note(cx, "sort by size"))
                        .entry(),
                ],
            ),
            MenuEntry::Separator,
            MenuItem::new(self.t("Delete", "刪除"))
                .icon(IconName::Delete)
                .shortcut("Del")
                .on_select(Self::note(cx, "delete"))
                .entry(),
        ]
    }

    fn dialog_buttons(&self) -> DialogButtons {
        DialogButtons {
            primary: Some(self.t("Save", "儲存").into()),
            secondary: Some(self.t("Don't save", "不要儲存").into()),
            close: Some(self.t("Cancel", "取消").into()),
            default: DefaultButton::Primary,
        }
    }

    fn dialogs_page(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let result = match self.dialog_result {
            Some(DialogResult::Primary) => self.t("Last result: Save", "上次結果：儲存"),
            Some(DialogResult::Secondary) => {
                self.t("Last result: Don't save", "上次結果：不要儲存")
            }
            Some(DialogResult::Close) => {
                self.t("Last result: Cancel or Esc", "上次結果：取消或 Esc")
            }
            None => self.t(
                "Enter presses Save, Esc cancels.",
                "Enter 會按下「儲存」，Esc 會取消。",
            ),
        };
        let dialog_row = line(
            self.t("Content dialog", "內容對話方塊"),
            p,
            div()
                .flex()
                .items_start()
                .gap(spacing::XL)
                .child(
                    ContentDialog::new(
                        "dialog-inline",
                        self.t("Save your work?", "要儲存您的工作嗎？"),
                    )
                    .buttons(self.dialog_buttons())
                    .inline()
                    .child(self.t(
                        "Your changes to Notes will be lost if you don't save them.",
                        "若不儲存，對「筆記」所做的變更將會遺失。",
                    )),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::SM)
                        .child(
                            div().flex().child(
                                Button::accent(
                                    "dialog-open",
                                    self.t("Open the dialog", "開啟對話方塊"),
                                )
                                .on_click(Self::act(cx, |this, _, _| this.dialog_open = true)),
                            ),
                        )
                        .child(label(result, CAPTION, p.text_secondary)),
                ),
        );
        let flyout = div()
            .flex()
            .items_start()
            .gap(spacing::XL)
            .child(
                Flyout::new("flyout-inline").inline().child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::MD)
                        .w(px(260.0))
                        .child(label(
                            self.t(
                                "All items will be removed. Do you want to continue?",
                                "將移除所有項目。要繼續嗎？",
                            ),
                            BODY,
                            p.text_primary,
                        ))
                        .child(div().flex().child(Button::accent(
                            "flyout-confirm",
                            self.t("Yes, empty my cart", "是，清空購物車"),
                        ))),
                ),
            )
            .child(
                div()
                    .relative()
                    .child(
                        Button::standard("flyout-open", self.t("Empty cart", "清空購物車"))
                            .on_click(Self::act(cx, |this, _, _| {
                                this.flyout_open = !this.flyout_open
                            })),
                    )
                    .when(self.flyout_open, |holder| {
                        holder.child(
                            Flyout::new("flyout-live")
                                .on_dismiss(Self::act(cx, |this, _, _| this.flyout_open = false))
                                .child(label(
                                    self.t(
                                        "A light-dismiss flyout. Esc or a click outside closes it.",
                                        "點外面或按 Esc 就會關閉的飛出視窗。",
                                    ),
                                    BODY,
                                    p.text_primary,
                                )),
                        )
                    }),
            );
        let open_context = Self::act_with(
            cx,
            |this: &mut Self, position: Point<Pixels>, window, cx| {
                this.context_at = position;
                open_menu(&this.context_menu, None, window, cx);
            },
        );
        let menu_states = div()
            .flex()
            .items_start()
            .gap(spacing::XL)
            .h(px(300.0))
            .child(
                MenuPanel::new(
                    "menu-states",
                    vec![
                        MenuItem::new(self.t("Rest", "一般"))
                            .icon(IconName::File)
                            .shortcut("Ctrl+N")
                            .shown(ShownState::Rest)
                            .entry(),
                        MenuItem::new(self.t("Pointer over", "指標停留"))
                            .icon(IconName::Folder)
                            .shortcut("Ctrl+O")
                            .shown(ShownState::PointerOver)
                            .entry(),
                        MenuItem::new(self.t("Pressed", "按下"))
                            .icon(IconName::Copy)
                            .shown(ShownState::Pressed)
                            .entry(),
                        MenuItem::new(self.t("Disabled", "停用"))
                            .icon(IconName::Delete)
                            .shortcut("Del")
                            .disabled(true)
                            .entry(),
                        MenuEntry::Separator,
                        MenuEntry::submenu(
                            self.t("Submenu, open", "子功能表（展開）"),
                            vec![
                                MenuItem::new(self.t("First", "第一項")).entry(),
                                MenuItem::new(self.t("Highlighted", "醒目提示")).entry(),
                                MenuEntry::Separator,
                                MenuItem::new(self.t("Last", "最後一項")).entry(),
                            ],
                        ),
                    ],
                )
                .open_submenu(5, Some(1)),
            )
            .child(
                div()
                    .ml(px(200.0))
                    .flex()
                    .flex_col()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .id("context-target")
                            .w(px(260.0))
                            .h(px(120.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(radius::SM)
                            .border_1()
                            .border_color(p.strong_stroke)
                            .text_color(p.text_secondary)
                            .child(self.t("Right-click here", "在這裡按右鍵"))
                            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                                open_context(event.position, window, cx)
                            }),
                    )
                    .child(label(
                        if self.last_action.is_empty() {
                            SharedString::from(self.t(
                                "Arrows move, Right opens a submenu, Esc closes.",
                                "方向鍵移動，向右鍵開啟子功能表，Esc 關閉。",
                            ))
                        } else {
                            SharedString::from(format!(
                                "{} {}",
                                self.t("Chose:", "選擇了："),
                                self.last_action
                            ))
                        },
                        CAPTION,
                        p.text_secondary,
                    )),
            );
        let tips = div()
            .flex()
            .items_center()
            .gap(spacing::XL)
            .child(tooltip_inline(self.t("Save (Ctrl+S)", "儲存 (Ctrl+S)"), cx))
            .child(
                Button::standard(
                    "tooltip-live",
                    self.t("Hover for a tooltip", "停留以顯示工具提示"),
                )
                .tooltip(self.t(
                    "A WinUI tooltip: 12px text, 4px corners",
                    "WinUI 工具提示：12px 文字、4px 圓角",
                )),
            );
        let mut rows = vec![
            section(self.t("Dialogs", "對話方塊"), p, vec![dialog_row]),
            section(
                self.t("Flyout", "飛出視窗"),
                p,
                vec![line(self.t("Flyout", "飛出視窗"), p, flyout)],
            ),
            section(
                self.t("Menu flyout", "功能表飛出視窗"),
                p,
                vec![line(
                    self.t("States, live", "狀態與實際操作"),
                    p,
                    menu_states,
                )],
            ),
            section(
                self.t("Tooltip", "工具提示"),
                p,
                vec![line(self.t("Tooltip", "工具提示"), p, tips)],
            ),
        ];
        if self.context_menu.read(cx).is_open() {
            rows.push(
                ContextMenu::new(
                    "context-menu",
                    self.menu_entries(cx),
                    self.context_menu.clone(),
                    self.context_at,
                )
                .into_any_element(),
            );
        }
        rows
    }

    fn collections_page_base(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let items = vec![
            ListItem::new(self.t("Rest", "一般"))
                .icon(IconName::Folder)
                .caption(self.t("Modified today", "今天修改")),
            ListItem::new(self.t("Selected, focused", "已選取、已聚焦"))
                .icon(IconName::Folder)
                .caption(self.t("12 items", "12 個項目")),
            ListItem::new(self.t("Pointer over", "指標停留"))
                .icon(IconName::File)
                .shown(ShownState::PointerOver),
            ListItem::new(self.t("Pressed", "按下"))
                .icon(IconName::File)
                .shown(ShownState::Pressed),
            ListItem::new(self.t("Disabled", "停用"))
                .icon(IconName::File)
                .disabled(true),
        ];
        let live_items = vec![
            ListItem::new("Documents")
                .icon(IconName::Folder)
                .caption(self.t("Folder", "資料夾")),
            ListItem::new("Downloads")
                .icon(IconName::Folder)
                .caption(self.t("Folder", "資料夾")),
            ListItem::new("notes.txt")
                .icon(IconName::File)
                .caption("2.4 KB"),
            ListItem::new("photo.png")
                .icon(IconName::File)
                .caption("1.1 MB"),
        ];
        let rows: Vec<Vec<SharedString>> = vec![
            vec!["report10.pdf".into(), "1.2 MB".into(), "2026-11-02".into()],
            vec!["report2.pdf".into(), "640 KB".into(), "2026-10-28".into()],
            vec!["Budget.xlsx".into(), "88 KB".into(), "2026-11-10".into()],
            vec![
                "archive.tar.zst".into(),
                "3.4 GB".into(),
                "2026-09-14".into(),
            ],
            vec!["README.md".into(), "4 KB".into(), "2026-11-11".into()],
        ];
        let columns = vec![
            Column::new(self.t("Name", "名稱")),
            Column::new(self.t("Size", "大小"))
                .width(px(120.0))
                .align_end(),
            Column::new(self.t("Modified", "修改日期")).width(px(160.0)),
        ];
        vec![
            section(
                self.t("List rows", "清單列"),
                p,
                vec![
                    line(
                        self.t("States", "狀態"),
                        p,
                        div().w(px(360.0)).child(
                            ListView::new("list-states", items, Some(1)).shown(ShownState::Focused),
                        ),
                    ),
                    line(
                        self.t("Live (Up, Down)", "實際操作（上下鍵）"),
                        p,
                        div().w(px(360.0)).child(
                            ListView::new("list-live", live_items, self.list).on_select(
                                Self::act_with(cx, |this, index, _, _| this.list = Some(index)),
                            ),
                        ),
                    ),
                ],
            ),
            section(
                self.t("Details table", "詳細資料表格"),
                p,
                vec![line(
                    self.t("Click a header to sort", "按標題排序"),
                    p,
                    div().w(px(620.0)).child(
                        DetailsTable::new("table", columns, rows)
                            .sort(self.table_sort)
                            .selected(self.table_selected)
                            .on_sort(Self::act_with(cx, |this, sort, _, _| {
                                this.table_sort = Some(sort)
                            }))
                            .on_select(Self::act_with(cx, |this, index, _, _| {
                                this.table_selected = Some(index)
                            })),
                    ),
                )],
            ),
            section(
                self.t("Application icons", "應用程式圖示"),
                p,
                vec![line(
                    self.t("Icon, or its letter", "圖示，或其首字母"),
                    p,
                    self.application_rows(p),
                )],
            ),
        ]
    }

    /// `better_ui::application_list_row_with_icon`: an icon named by a
    /// desktop entry, resolved from the icon theme, and one that is not
    /// installed, which falls back to its letter.
    fn application_rows(&self, p: &Palette) -> impl IntoElement {
        let search = better_ui::icons::IconSearch::from_environment();
        let style = better_ui::TileStyle {
            foreground: p.text_primary.into(),
            muted_foreground: p.text_secondary.into(),
            border: p.card_stroke.into(),
            background: p.card.into(),
            glyph_foreground: p.text_on_accent.into(),
            glyph_background: p.accent.into(),
            radius: radius::SM,
        };
        let row = |icon: Option<&str>, name: &'static str, detail: &'static str| {
            better_ui::application_list_row_with_icon(
                better_ui::AppIcon::for_entry(icon, name, &search),
                name,
                detail,
                Vec::new(),
                style,
            )
        };
        div()
            .w(px(420.0))
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .child(row(
                Some("better-files"),
                "Better Files",
                self.t(
                    "Icon=better-files, from the theme",
                    "Icon=better-files，取自佈景主題",
                ),
            ))
            .child(row(
                Some("not-installed-anywhere"),
                "Notes",
                self.t("No icon file, so its letter", "找不到圖示檔，改用首字母"),
            ))
    }

    fn nav_entries(&self) -> Vec<NavEntry> {
        vec![
            NavEntry::Header(self.t("Library", "媒體櫃").into()),
            NavItem::new(self.t("Home", "首頁"), IconName::LayoutDashboard).entry(),
            NavItem::new(self.t("Selected", "已選取"), IconName::Folder).entry(),
            NavItem::new(self.t("Pointer over", "指標停留"), IconName::Star)
                .shown(ShownState::PointerOver)
                .entry(),
            NavItem::new(self.t("Pressed", "按下"), IconName::Globe)
                .shown(ShownState::Pressed)
                .entry(),
            NavItem::new(self.t("Disabled", "停用"), IconName::HardDrive)
                .disabled(true)
                .entry(),
            NavEntry::Separator,
            NavItem::new(self.t("Trash", "資源回收筒"), IconName::Delete).entry(),
        ]
    }

    /// Fixture settings for the auto-suggest demos, matched by the start of
    /// any word, as the control center's search does.
    fn suggestions_for(&self, query: &str) -> Vec<(usize, Suggestion)> {
        let all = [
            (
                self.t("Battery charge limit", "電池充電上限"),
                self.t("System › Power", "系統 › 電源"),
            ),
            (
                self.t("Power mode", "電源模式"),
                self.t("System › Power", "系統 › 電源"),
            ),
            (
                self.t("Brightness", "螢幕亮度"),
                self.t("System › Display", "系統 › 顯示"),
            ),
            (self.t("Appearance", "外觀"), self.t("Settings", "設定")),
            (self.t("Language", "語言"), self.t("Settings", "設定")),
        ];
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        all.iter()
            .enumerate()
            .filter(|(_, (title, _))| {
                let title = title.to_lowercase();
                title
                    .split_whitespace()
                    .any(|word| word.starts_with(&query))
                    || (!title.is_ascii() && title.contains(&query))
            })
            .map(|(index, (title, caption))| (index, Suggestion::new(*title).caption(*caption)))
            .collect()
    }

    fn suggest_box(
        &self,
        id: &'static str,
        state: &Entity<InputState>,
        width: Option<Pixels>,
        cx: &Context<Self>,
    ) -> AutoSuggestBox {
        let query = state.read(cx).value().to_string();
        let found = self.suggestions_for(&query);
        let picked: Vec<usize> = found.iter().map(|(index, _)| *index).collect();
        let empty = if self.zh {
            format!("找不到符合「{query}」的設定。")
        } else {
            format!("No settings match \"{query}\".")
        };
        let mut field = AutoSuggestBox::new(id, state)
            .suggestions(found.into_iter().map(|(_, s)| s).collect())
            .empty_text(empty)
            .on_choose(Self::act_with(cx, move |this, index: usize, _, _| {
                this.last_action = format!("chose suggestion {}", picked[index]).into();
            }))
            .on_submit(Self::note(cx, "submitted the text"));
        if let Some(width) = width {
            field = field.width(width);
        }
        field
    }

    fn navigation_page(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let footer = || vec![NavItem::new(self.t("Settings", "設定"), IconName::Settings)];
        let frame = |compact: bool| {
            div()
                .w(px(if compact { 240.0 } else { 480.0 }))
                .h(px(400.0))
                .rounded(radius::SM)
                .border_1()
                .border_color(p.card_stroke)
                .overflow_hidden()
        };
        let demo = |compact: bool, id: &'static str| {
            frame(compact).child(
                NavigationView::new(id, self.nav_entries(), Some(2))
                    .footer(footer())
                    .compact(compact)
                    .title("Better Files")
                    .locale(self.locale())
                    .shown(ShownState::Focused)
                    .content(div().p(spacing::LG).child(label(
                        self.t("Content", "內容"),
                        BODY,
                        p.text_secondary,
                    ))),
            )
        };
        let live_nav = frame(self.demo_compact).child(
            NavigationView::new("nav-live", self.nav_entries(), self.demo_nav)
                .footer(footer())
                .compact(self.demo_compact)
                .title(self.t("Live", "實際操作"))
                .locale(self.locale())
                .on_select(Self::act_with(cx, |this, index, _, _| {
                    this.demo_nav = Some(index)
                }))
                .on_toggle(Self::act_with(cx, |this, compact, _, _| {
                    this.demo_compact = compact
                }))
                .content(div().p(spacing::LG).child(label(
                    self.t(
                        "Up and Down move, Enter selects.",
                        "上下鍵移動，Enter 選取。",
                    ),
                    BODY,
                    p.text_secondary,
                ))),
        );
        let tabs: Vec<Tab> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, title)| {
                let tab = Tab::new(*title).icon(IconName::File);
                if index == 2 {
                    tab.shown(ShownState::PointerOver)
                } else {
                    tab
                }
            })
            .collect();
        let selector_items = vec![
            SelectorItem::new(self.t("Recent", "最近")).icon(IconName::Undo),
            SelectorItem::new(self.t("Shared", "已共用")).icon(IconName::User),
            SelectorItem::new(self.t("Favorites", "我的最愛")).icon(IconName::Star),
            SelectorItem::new(self.t("Unavailable", "無法使用")).disabled(true),
        ];
        let selector_states = vec![
            SelectorItem::new(self.t("Selected, focused", "已選取、已聚焦")),
            SelectorItem::new(self.t("Pointer over", "指標停留")).shown(ShownState::PointerOver),
            SelectorItem::new(self.t("Pressed", "按下")).shown(ShownState::Pressed),
            SelectorItem::new(self.t("Disabled", "停用")).disabled(true),
        ];
        let crumbs: Vec<SharedString> =
            self.crumbs.iter().map(|c| SharedString::from(*c)).collect();
        let command_bar = |show_overflow: bool, id: &'static str| {
            let bar = CommandBar::new(id)
                .locale(self.locale())
                .command(
                    Command::new(self.t("New", "新增"), IconName::Plus)
                        .on_click(Self::note(cx, "new")),
                )
                .command(
                    Command::new(self.t("Copy", "複製"), IconName::Copy)
                        .shown(ShownState::PointerOver),
                )
                .command(
                    Command::new(self.t("Rename", "重新命名"), IconName::Replace)
                        .shown(ShownState::Pressed),
                )
                .command(Command::new(self.t("Delete", "刪除"), IconName::Delete).disabled(true))
                .separator()
                .command(
                    Command::new(self.t("Details", "詳細資料"), IconName::PanelRight).checked(true),
                )
                .command(Command::new(self.t("Sort", "排序"), IconName::SortAscending).icon_only())
                .secondary(
                    Command::new(self.t("Select all", "全選"), IconName::Check)
                        .shortcut("Ctrl+A")
                        .on_click(Self::note(cx, "select all")),
                )
                .secondary(Command::new(self.t("Properties", "內容"), IconName::Info));
            if show_overflow {
                bar.show_overflow()
            } else {
                bar
            }
        };
        let badged = || {
            vec![
                NavItem::new(self.t("Home", "首頁"), IconName::LayoutDashboard).entry(),
                NavItem::new(self.t("Keep awake", "保持清醒"), IconName::Sun)
                    .badge(Some(InfoBadge::Dot))
                    .entry(),
                NavItem::new(self.t("Apps", "應用程式"), IconName::Inbox)
                    .badge(InfoBadge::count(2))
                    .entry(),
                NavItem::new(self.t("Fixes", "修正"), IconName::TriangleAlert)
                    .badge(InfoBadge::count(120))
                    .entry(),
            ]
        };
        let search_demo = |compact: bool, id: &'static str| {
            frame(compact).child(
                NavigationView::new(id, badged(), Some(0))
                    .footer(footer())
                    .compact(compact)
                    .locale(self.locale())
                    .search(
                        div().child(label(
                            self.t("Find a setting", "尋找設定"),
                            BODY,
                            p.text_secondary,
                        )),
                        self.t("Find a setting", "尋找設定"),
                    )
                    .content(div().p(spacing::LG).child(label(
                        self.t("Content", "內容"),
                        BODY,
                        p.text_secondary,
                    ))),
            )
        };
        let search_live = div()
            .w(px(720.0))
            .h(px(420.0))
            .rounded(radius::SM)
            .border_1()
            .border_color(p.card_stroke)
            .overflow_hidden()
            .child(
                NavigationView::new("nav-search-live", badged(), self.search_nav)
                    .footer(footer())
                    .compact(self.search_compact)
                    .overlay(self.search_overlay)
                    .locale(self.locale())
                    .search(
                        self.suggest_box("pane-suggest", &self.pane_search_input, None, cx),
                        self.t("Find a setting", "尋找設定"),
                    )
                    .on_search_open(Self::act(cx, |this, window, cx| {
                        let input = this.pane_search_input.clone();
                        input.update(cx, |input, cx| input.focus(window, cx));
                    }))
                    .on_select(Self::act_with(cx, |this, index, _, _| {
                        this.search_nav = Some(index)
                    }))
                    .on_toggle(Self::act_with(cx, |this, compact, _, _| {
                        this.search_compact = compact
                    }))
                    .content(
                        div()
                            .p(spacing::LG)
                            .flex()
                            .flex_col()
                            .gap(spacing::SM)
                            .child(label(
                                self.t(
                                    "☰ opens the pane over this content; a press outside closes it.",
                                    "☰ 會把窗格展開在內容上方，按一下外面就收合。",
                                ),
                                BODY,
                                p.text_secondary,
                            ))
                            .child(
                                ToggleSwitch::new("overlay-switch", self.search_overlay)
                                    .header(self.t("Open over the content", "展開時蓋在內容上"))
                                    .locale(self.locale())
                                    .on_change(Self::act_with(cx, |this, on, _, _| {
                                        this.search_overlay = on
                                    })),
                            )
                            .child(label(
                                self.last_action.clone(),
                                CAPTION,
                                p.text_secondary,
                            )),
                    ),
            );
        vec![
            section(
                self.t(
                    "Navigation view with search and badges",
                    "導覽檢視：搜尋與徽章",
                ),
                p,
                vec![
                    line(
                        self.t("Open and compact", "展開與精簡"),
                        p,
                        div()
                            .flex()
                            .gap(spacing::LG)
                            .child(search_demo(false, "nav-search-open"))
                            .child(search_demo(true, "nav-search-compact")),
                    ),
                    line(
                        self.t(
                            "Live (type, ↑↓, Enter, Esc)",
                            "實際操作（輸入、↑↓、Enter、Esc）",
                        ),
                        p,
                        search_live,
                    ),
                    line(
                        self.t("Info badge", "資訊徽章"),
                        p,
                        div()
                            .flex()
                            .items_center()
                            .gap(spacing::LG)
                            .child(Badge::new(InfoBadge::Dot))
                            .child(Badge::new(InfoBadge::Count(2)))
                            .child(Badge::new(InfoBadge::Count(42)))
                            .child(Badge::new(InfoBadge::Count(120))),
                    ),
                    line(
                        self.t("Auto-suggest box", "自動建議方塊"),
                        p,
                        self.suggest_box("suggest-live", &self.suggest_input, Some(px(320.0)), cx),
                    ),
                ],
            ),
            section(
                self.t("Navigation view", "導覽檢視"),
                p,
                vec![
                    line(
                        self.t("Open and compact (focused)", "展開與精簡（已聚焦）"),
                        p,
                        div()
                            .flex()
                            .gap(spacing::LG)
                            .child(demo(false, "nav-open"))
                            .child(demo(true, "nav-compact")),
                    ),
                    line(
                        self.t("Live (☰ toggles)", "實際操作（☰ 切換）"),
                        p,
                        live_nav,
                    ),
                ],
            ),
            section(
                self.t("Command bar", "命令列"),
                p,
                vec![
                    line(
                        self.t("States", "狀態"),
                        p,
                        div().w(px(820.0)).child(command_bar(false, "bar-live")),
                    ),
                    line(
                        self.t("Overflow open", "溢位選單展開"),
                        p,
                        div()
                            .w(px(820.0))
                            .h(px(150.0))
                            .child(command_bar(true, "bar-open")),
                    ),
                ],
            ),
            section(
                self.t(
                    "Breadcrumb, tabs, selector bar",
                    "階層路徑、索引標籤、選取器列",
                ),
                p,
                vec![
                    line(
                        self.t("Breadcrumb bar", "階層路徑列"),
                        p,
                        div()
                            .flex()
                            .flex_col()
                            .gap(spacing::SM)
                            .child(BreadcrumbBar::new("crumbs", crumbs.clone()).on_click(
                                Self::act_with(cx, |this, index: usize, _, _| {
                                    this.crumbs.truncate(index + 1)
                                }),
                            ))
                            .child(
                                BreadcrumbBar::new("crumbs-folded", crumbs)
                                    .max_visible(3)
                                    .style(SUBTITLE),
                            )
                            .child(
                                div().flex().child(
                                    Button::subtle(
                                        "crumbs-reset",
                                        self.t("Reset the path", "重設路徑"),
                                    )
                                    .on_click(Self::act(cx, |this, _, _| {
                                        this.crumbs = CRUMBS.to_vec()
                                    })),
                                ),
                            ),
                    ),
                    line(
                        self.t("Tab view", "索引標籤檢視"),
                        p,
                        div()
                            .w(px(620.0))
                            .flex()
                            .flex_col()
                            .child(
                                TabView::new("tabs", tabs, self.tab)
                                    .on_select(Self::act_with(cx, |this, index, _, _| {
                                        this.tab = Some(index)
                                    }))
                                    .on_close(Self::act_with(cx, |this, index: usize, _, _| {
                                        let len = this.tabs.len();
                                        this.tab = after_close(this.tab, index, len);
                                        this.tabs.remove(index);
                                    }))
                                    .on_add(Self::act(cx, |this, _, _| {
                                        this.tabs.push("untitled.txt");
                                        this.tab = Some(this.tabs.len() - 1);
                                    })),
                            )
                            .child(
                                div()
                                    .h(px(60.0))
                                    .p(spacing::MD)
                                    .bg(p.tab_selected)
                                    .border_1()
                                    .border_t_0()
                                    .border_color(p.card_stroke)
                                    .child(label(
                                        self.tab
                                            .and_then(|i| self.tabs.get(i))
                                            .copied()
                                            .unwrap_or("—"),
                                        BODY,
                                        p.text_secondary,
                                    )),
                            ),
                    ),
                    line(
                        self.t("Selector bar", "選取器列"),
                        p,
                        div()
                            .flex()
                            .flex_col()
                            .gap(spacing::SM)
                            .child(
                                SelectorBar::new("selector", selector_items, self.selector)
                                    .on_select(Self::act_with(cx, |this, index, _, _| {
                                        this.selector = Some(index)
                                    })),
                            )
                            .child(
                                SelectorBar::new("selector-states", selector_states, Some(0))
                                    .shown(ShownState::Focused),
                            ),
                    ),
                ],
            ),
        ]
    }

    /// The settings cards, then the status card and the device banner the
    /// control center's Home is built from.
    fn settings_page(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut sections = self.settings_page_base(p, cx);
        let status = |severity: Severity, id: &'static str, header: &'static str| {
            SettingsCard::new(id, header)
                .status(severity)
                .description(self.t("A card that reports a state", "回報狀態的卡片"))
                .into_any_element()
        };
        sections.push(section(
            self.t("Status cards and the device banner", "狀態卡片與裝置橫幅"),
            p,
            vec![
                line(
                    self.t("Status card", "狀態卡片"),
                    p,
                    div()
                        .w(px(640.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(status(
                            Severity::Success,
                            "status-success",
                            self.t("Everything's fine", "一切正常"),
                        ))
                        .child(status(
                            Severity::Informational,
                            "status-info",
                            self.t("Informational", "資訊"),
                        ))
                        .child(status(
                            Severity::Warning,
                            "status-warning",
                            self.t("Warning", "警告"),
                        ))
                        .child(status(
                            Severity::Error,
                            "status-error",
                            self.t("Error", "錯誤"),
                        )),
                ),
                line(
                    self.t("Device banner", "裝置橫幅"),
                    p,
                    div().w(px(640.0)).child(
                        DeviceBanner::new(IconName::PanelBottom, "ASUS Vivobook S 14 M5406WA")
                            .caption("Zorin OS 18.1 · Better OS 0.2.11")
                            .trailing(label(
                                self.t("87% · Charging", "87% · 充電中"),
                                BODY,
                                p.text_secondary,
                            )),
                    ),
                ),
            ],
        ));
        sections
    }

    /// The lists and tables, then rows with a status glyph, as an activity
    /// log draws them.
    fn collections_page(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut sections = self.collections_page_base(p, cx);
        sections.push(section(
            self.t("Rows with a status glyph", "帶狀態圖示的列"),
            p,
            vec![line(
                self.t("Activity", "活動紀錄"),
                p,
                div()
                    .w(px(480.0))
                    .flex()
                    .flex_col()
                    .child(
                        ListRow::new(
                            "status-row-ok",
                            self.t("Better Files · Update", "Better Files · 更新"),
                        )
                        .status_glyph(IconName::CircleCheck, Severity::Success)
                        .caption(self.t("Passed · 1 day ago", "通過 · 1 天前")),
                    )
                    .child(
                        ListRow::new(
                            "status-row-warn",
                            self.t("Better Monitor · Check", "Better Monitor · 系統檢查"),
                        )
                        .status_glyph(IconName::TriangleAlert, Severity::Warning)
                        .caption(self.t(
                            "Service not running · 2 h ago",
                            "背景服務沒有在執行 · 2 小時前",
                        )),
                    )
                    .child(
                        ListRow::new(
                            "status-row-fail",
                            self.t("Better Awake · Install", "Better Awake · 安裝"),
                        )
                        .status_glyph(IconName::CircleX, Severity::Error)
                        .caption(self.t("Installing files · 21 min ago", "安裝檔案 · 21 分鐘前")),
                    )
                    .child(
                        ListRow::new(
                            "status-row-info",
                            self.t("Better Manager · Check", "Better Manager · 系統檢查"),
                        )
                        .status_glyph(IconName::Info, Severity::Informational)
                        .caption(self.t("Just now", "剛剛")),
                    ),
            )],
        ));
        sections.push(section(
            self.t("Label chips and a branch graph", "標籤晶片與分支圖"),
            p,
            vec![
                line(
                    self.t("Chips", "標籤晶片"),
                    p,
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::SM)
                        .child(Chip::new("main").hue(lane_hue(0)).emphasized(true))
                        .child(Chip::new("feature/login").hue(lane_hue(1)))
                        .child(
                            Chip::new("origin/main")
                                .icon(IconName::Globe)
                                .hue(lane_hue(0)),
                        )
                        .child(Chip::new("v1.0")),
                ),
                line(self.t("Branch graph", "分支圖"), p, graph_demo(p)),
            ],
        ));
        sections
    }

    fn settings_page_base(&self, p: &Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let combo = ComboBox::new(
            "settings-combo",
            vec![
                ComboItem::new(self.t("Use system setting", "跟隨系統")),
                ComboItem::new(self.t("Light", "淺色")),
                ComboItem::new(self.t("Dark", "深色")),
            ],
            Appearance::ALL
                .iter()
                .position(|choice| *choice == Appearance::current(cx)),
        )
        .on_select(Self::act_with(cx, |this, index: usize, window, cx| {
            if let Some(choice) = Appearance::ALL.get(index) {
                this.set_appearance(*choice, window, cx);
            }
        }));
        let cards = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .w(px(820.0))
            .child(
                SettingsCard::new("card-switch", self.t("Bluetooth", "藍牙"))
                    .icon(IconName::Network)
                    .description(self.t(
                        "Discoverable as \"Tim's laptop\"",
                        "可被搜尋為「Tim 的筆電」",
                    ))
                    .content(
                        ToggleSwitch::new("card-switch-control", self.switch_on)
                            .locale(self.locale())
                            .on_change(Self::act_with(cx, |this, value, _, _| {
                                this.switch_on = value
                            })),
                    ),
            )
            .child(
                SettingsCard::new("card-combo", self.t("Choose your mode", "選擇模式"))
                    .icon(IconName::Palette)
                    .description(self.t(
                        "Change the colors that appear in Better OS windows",
                        "變更 Better OS 視窗中的色彩",
                    ))
                    .content(combo),
            )
            .child(
                SettingsCard::new("card-click", self.t("Storage", "儲存空間"))
                    .icon(IconName::HardDrive)
                    .description(self.t(
                        "Storage space, drives, configuration rules",
                        "儲存空間、磁碟機、設定規則",
                    ))
                    .on_click(Self::note(cx, "storage")),
            )
            .child(
                SettingsCard::new("card-hover", self.t("Pointer over", "指標停留"))
                    .icon(IconName::Bell)
                    .description(self.t("A card that opens a page", "會開啟另一頁的卡片"))
                    .shown(ShownState::PointerOver)
                    .on_click(|_, _| {}),
            )
            .child(
                SettingsCard::new("card-pressed", self.t("Pressed", "按下"))
                    .icon(IconName::Star)
                    .shown(ShownState::Pressed)
                    .on_click(|_, _| {}),
            )
            .child(
                SettingsCard::new("card-focus", self.t("Focused", "已聚焦"))
                    .icon(IconName::User)
                    .shown(ShownState::Focused)
                    .on_click(|_, _| {}),
            )
            .child(
                SettingsCard::new("card-disabled", self.t("Disabled", "已停用"))
                    .icon(IconName::Moon)
                    .description(self.t("Not available on this device", "此裝置無法使用"))
                    .disabled(true)
                    .content(
                        ToggleSwitch::new("card-disabled-switch", false)
                            .locale(self.locale())
                            .disabled(true),
                    ),
            )
            .child(
                SettingsExpander::new(
                    "expander",
                    self.t("Sound output", "聲音輸出"),
                    self.expander,
                )
                .icon(IconName::Bell)
                .description(self.t("Choose where to play sound", "選擇播放聲音的位置"))
                .on_toggle(Self::act_with(cx, |this, open, _, _| this.expander = open))
                .row(
                    SettingsRow::new(self.t("Volume", "音量")).content(
                        Slider::new("expander-slider", self.slider, SliderModel::new(0.0, 100.0))
                            .on_change(Self::act_with(cx, |this, value, _, _| this.slider = value)),
                    ),
                )
                .row(
                    SettingsRow::new(self.t("Mono audio", "單聲道音訊"))
                        .description(self.t(
                            "Combine left and right channels into one",
                            "將左右聲道合併為一個",
                        ))
                        .content(ToggleSwitch::new("expander-switch", false).locale(self.locale())),
                ),
            )
            .child(
                SettingsExpander::new(
                    "expander-closed",
                    self.t("Closed, pointer over", "收合（指標停留）"),
                    false,
                )
                .icon(IconName::Settings)
                .shown(ShownState::PointerOver),
            );
        // Settings cards sit on the page, not on another card.
        vec![
            label(self.t("Settings", "設定"), SUBTITLE, p.text_primary).into_any_element(),
            cards.into_any_element(),
        ]
    }

    fn window_page(&self, p: &Palette) -> Vec<AnyElement> {
        let icon = || {
            Icon::new(IconName::Frame)
                .size(px(16.0))
                .text_color(p.accent)
        };
        // The header and its buttons are the desktop's (ticket 108): GTK draws
        // them from the current theme. Each sample is drawn in one appearance
        // with a strip of that appearance's window under it.
        let frame = |bar: TitleBar, dark: bool| {
            div().w(px(720.0)).child(bar.appearance(dark)).child(
                div()
                    .h(px(24.0))
                    .bg(better_ui::fluent::palette_for(dark).window_background),
            )
        };
        let title = self.t("Better Files — Documents", "Better Files — 文件");
        let bar = || TitleBar::new(title).icon(icon());
        let right = || ButtonLayout::parse("appmenu:minimize,maximize,close");
        let left = || ButtonLayout::parse("close,maximize,minimize:appmenu");
        vec![section(
            self.t("Titlebar and type", "標題列與字型"),
            p,
            vec![
                line(
                    self.t("Buttons on the right, light", "按鈕在右側（淺色）"),
                    p,
                    frame(bar().layout(right()), false),
                ),
                line(
                    self.t("Buttons on the right, dark", "按鈕在右側（深色）"),
                    p,
                    frame(bar().layout(right()), true),
                ),
                line(
                    self.t("Buttons on the left, light", "按鈕在左側（淺色）"),
                    p,
                    frame(bar().layout(left()), false),
                ),
                line(
                    self.t("Buttons on the left, dark", "按鈕在左側（深色）"),
                    p,
                    frame(bar().layout(left()), true),
                ),
                line(
                    self.t("Close only", "只有關閉"),
                    p,
                    frame(
                        bar().layout(ButtonLayout::parse("appmenu:close")),
                        p.is_dark(),
                    ),
                ),
                line(
                    self.t("The desktop's layout", "桌面目前的配置"),
                    p,
                    frame(bar(), p.is_dark()),
                ),
                line(
                    self.t("Back button", "返回按鈕"),
                    p,
                    frame(
                        bar().layout(right()).back(self.locale(), |_, _| {}),
                        p.is_dark(),
                    ),
                ),
                line(
                    self.t("Minimize, pointer over", "最小化（指標停留）"),
                    p,
                    frame(
                        bar()
                            .layout(right())
                            .shown(WindowButton::Minimize, ShownState::PointerOver),
                        p.is_dark(),
                    ),
                ),
                line(
                    self.t("Maximize, pressed", "最大化（按下）"),
                    p,
                    frame(
                        bar()
                            .layout(right())
                            .shown(WindowButton::Maximize, ShownState::Pressed),
                        p.is_dark(),
                    ),
                ),
                line(
                    self.t("Close, pointer over", "關閉（指標停留）"),
                    p,
                    frame(
                        bar()
                            .layout(right())
                            .shown(WindowButton::Close, ShownState::PointerOver),
                        p.is_dark(),
                    ),
                ),
                line(
                    self.t("Close, pressed", "關閉（按下）"),
                    p,
                    frame(
                        bar()
                            .layout(right())
                            .shown(WindowButton::Close, ShownState::Pressed),
                        p.is_dark(),
                    ),
                ),
                line(
                    self.t("Inactive window", "非作用中視窗"),
                    p,
                    frame(bar().layout(right()).inactive(), p.is_dark()),
                ),
                line(
                    self.t("Type ramp", "字型層級"),
                    p,
                    div()
                        .flex()
                        .flex_col()
                        .gap(spacing::XS)
                        .child(label(
                            self.t("Title 28/36", "標題 28/36"),
                            TITLE,
                            p.text_primary,
                        ))
                        .child(label(
                            self.t("Subtitle 20/28", "副標題 20/28"),
                            SUBTITLE,
                            p.text_primary,
                        ))
                        .child(label(
                            self.t("Body Strong 14/20", "內文粗體 14/20"),
                            BODY_STRONG,
                            p.text_primary,
                        ))
                        .child(label(
                            self.t("Body 14/20", "內文 14/20"),
                            BODY,
                            p.text_primary,
                        ))
                        .child(label(
                            self.t("Caption 12/16", "說明 12/16"),
                            CAPTION,
                            p.text_secondary,
                        )),
                ),
            ],
        )]
    }
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let page = self.page;
        let content: Vec<AnyElement> = match page {
            Page::Buttons => self.buttons_page(p, cx),
            Page::Text => self.text_page(p, cx),
            Page::Selection => self.selection_page(p, cx),
            Page::Status => self.status_page(p, cx),
            Page::Dialogs => self.dialogs_page(p, cx),
            Page::Collections => self.collections_page(p, cx),
            Page::Navigation => self.navigation_page(p, cx),
            Page::Settings => self.settings_page(p, cx),
            Page::Window => self.window_page(p),
        };
        let shown_dark = shows_dark(cx);
        let bar = CommandBar::new("gallery-bar")
            .locale(self.locale())
            .command(
                Command::new(
                    if shown_dark {
                        self.t("Light", "淺色")
                    } else {
                        self.t("Dark", "深色")
                    },
                    if shown_dark {
                        IconName::Sun
                    } else {
                        IconName::Moon
                    },
                )
                .on_click(Self::act(cx, |this, window, cx| {
                    // Switches to the other one for good, as a person
                    // looking at the opposite appearance means to.
                    let next = if shows_dark(cx) {
                        Appearance::Light
                    } else {
                        Appearance::Dark
                    };
                    this.set_appearance(next, window, cx);
                })),
            )
            .command(
                Command::new(if self.zh { "English" } else { "中文" }, IconName::Globe)
                    .on_click(Self::act(cx, |this, _, _| this.zh = !this.zh)),
            );
        let page_view = div().id("page").size_full().overflow_y_scroll().child(
            div()
                .flex()
                .flex_col()
                .gap(spacing::XL)
                .px(spacing::XL)
                .pb(spacing::XL)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .pt(spacing::LG)
                        .child(div().flex_1().child(label(
                            page.title(self.zh),
                            TITLE,
                            p.text_primary,
                        )))
                        .child(bar),
                )
                .children(content),
        );
        let entries: Vec<NavEntry> = PAGES[..8]
            .iter()
            .map(|page| NavItem::new(page.title(self.zh), page.icon()).entry())
            .collect();
        let selected = PAGES.iter().position(|candidate| *candidate == page);
        let nav = NavigationView::new("gallery-nav", entries, selected)
            .footer(vec![NavItem::new(
                Page::Window.title(self.zh),
                Page::Window.icon(),
            )])
            .compact(self.compact)
            .title(self.t("Fluent controls", "Fluent 控制項"))
            .locale(self.locale())
            .on_select(Self::act_with(cx, |this, index: usize, _, _| {
                this.page = PAGES[index]
            }))
            .on_toggle(Self::act_with(cx, |this, compact, _, _| {
                this.compact = compact
            }))
            .content(page_view);

        let dialog = self.dialog_open.then(|| {
            ContentDialog::new(
                "dialog-live",
                self.t("Save your work?", "要儲存您的工作嗎？"),
            )
            .buttons(self.dialog_buttons())
            .on_result(Self::act_with(cx, |this, result, _, _| {
                this.dialog_result = Some(result);
                this.dialog_open = false;
            }))
            .child(self.t(
                "Your changes to Notes will be lost if you don't save them.",
                "若不儲存，對「筆記」所做的變更將會遺失。",
            ))
        });

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            // The window frame paints the window background, with the
            // desktop's top corners (ticket 108).
            .text_color(p.text_primary)
            .child(
                TitleBar::new(self.t("Better Fluent Gallery", "Better Fluent 控制項庫")).icon(
                    Icon::new(IconName::Palette)
                        .size(px(16.0))
                        .text_color(p.accent),
                ),
            )
            .child(div().flex_1().min_h_0().child(nav))
            .children(dialog)
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    COMMAND_LINE.open_or_exit(&arguments);
    let options = Rc::new(Cell::new(Some(options(&arguments))));
    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);
    app.run(move |cx| {
        gpui_component::init(cx);
        let window_options = gpui::WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1480.0), px(1000.0)), cx)),
            ..better_ui::window_chrome::window_options("io.betteros.FluentGallery")
        };
        let options = options.clone();
        cx.spawn(async move |cx| {
            cx.open_window(window_options, move |window, cx| {
                let options = options.take().expect("the gallery opens one window");
                let view = cx.new(|cx| Gallery::new(options, window, cx));
                cx.new(|cx| better_ui::window_chrome::root(view, window, cx))
            })
            .expect("failed to open the gallery window");
        })
        .detach();
    });
}
