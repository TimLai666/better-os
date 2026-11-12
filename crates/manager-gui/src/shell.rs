//! The control center's window around every page: the titlebar with its
//! back button, and the navigation pane with "Find a setting" (spec section
//! 1). The decisions — which pane item is selected, how wide the pane is,
//! what the search finds, what each badge says — are made in `navigation`,
//! `search`, and `pages`; this file only draws them.

use better_ui::fluent::auto_suggest::{AutoSuggestBox, Suggestion};
use better_ui::fluent::badge::InfoBadge;
use better_ui::fluent::chrome::TitleBar;
use better_ui::fluent::navigation::{NavItem, NavigationView};
use better_ui::fluent::text_box::TextBox;
use better_ui::fluent::{BODY, TITLE, label, palette, palette_for, spacing};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{Icon, IconName};

use crate::{
    app::ManagerApp,
    i18n::shell,
    navigation::{Route, is_narrow, pane_layout},
    pages::{self, NavBadge},
    search::{self, SearchEntry},
};

/// The width of the search box in the open pane: the pane less 16px on each
/// side (`NavigationViewAutoSuggestBoxMargin`).
const SEARCH_WIDTH: f32 = 288.0;

/// The pane icon for each item (spec section 1's table). Lucide has no house
/// or touchpad glyph, so Home and Touchpad use the nearest two.
pub(crate) fn route_icon(route: Route) -> IconName {
    match route {
        Route::Home => IconName::LayoutDashboard,
        Route::System => IconName::Cpu,
        Route::Touchpad => IconName::Frame,
        Route::KeepAwake => IconName::Sun,
        Route::Apps => IconName::Inbox,
        Route::DefaultApps => IconName::Star,
        Route::Fixes => IconName::TriangleAlert,
        Route::Settings => IconName::Settings,
    }
}

fn info_badge(badge: Option<NavBadge>) -> Option<InfoBadge> {
    match badge? {
        NavBadge::Dot => Some(InfoBadge::Dot),
        NavBadge::Count(count) => InfoBadge::count(count),
    }
}

impl ManagerApp {
    /// A kit handler that runs `f` on this window's state.
    pub(crate) fn act(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |window, cx| {
            let _ = weak.update(cx, |this, cx| f(this, window, cx));
        }
    }

    /// A kit handler told a value, that runs `f` on this window's state.
    pub(crate) fn act_with<T: 'static>(
        cx: &Context<Self>,
        f: impl Fn(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(T, &mut Window, &mut App) + 'static {
        let weak = cx.entity().downgrade();
        move |value, window, cx| {
            let _ = weak.update(cx, |this, cx| f(this, value, window, cx));
        }
    }

    /// The window's titlebar: `Better Manager` in both languages (audit
    /// M12), and a back button while there is somewhere to go back to.
    pub(crate) fn title_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let mut bar = TitleBar::new(shell(self.locale).window_title).icon(
            Icon::new(IconName::GalleryVerticalEnd)
                .size(px(16.0))
                .text_color(p.accent),
        );
        if crate::navigation::shows_back(&self.history, &self.page, self.transaction_running()) {
            bar = bar.back(
                self.locale.words(),
                Self::act(cx, |this, _, cx| this.go_back(cx)),
            );
        }
        bar.into_any_element()
    }

    /// What the pane's search box finds for the text it holds now.
    fn search_results(&self, cx: &App) -> (String, Vec<SearchEntry>) {
        let query = self.nav_search.read(cx).value().to_string();
        let index = self.search_index();
        let found = search::find(&index, &query).into_iter().cloned().collect();
        (query, found)
    }

    fn search_box(&self, cx: &mut Context<Self>) -> AutoSuggestBox {
        let s = shell(self.locale);
        let (query, found) = self.search_results(cx);
        let suggestions = found
            .iter()
            .map(|entry| Suggestion::new(entry.header).caption(entry.location()))
            .collect();
        let chosen = found.clone();
        let first = found.first().cloned();
        AutoSuggestBox::new("find-a-setting", &self.nav_search)
            .width(px(SEARCH_WIDTH))
            .suggestions(suggestions)
            .empty_text(s.search_no_result.replace("{query}", query.trim()))
            .on_choose(Self::act_with(cx, move |this, index: usize, window, cx| {
                if let Some(entry) = chosen.get(index) {
                    this.open_search_result(entry, window, cx);
                }
            }))
            .on_submit(Self::act(cx, move |this, window, cx| {
                // Enter with nothing highlighted opens the best match.
                if let Some(entry) = first.as_ref() {
                    this.open_search_result(entry, window, cx);
                }
            }))
    }

    /// The pane and the page beside it.
    pub(crate) fn navigation(
        &self,
        content: AnyElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = shell(self.locale);
        let width = f32::from(window.viewport_size().width);
        let narrow = is_narrow(width);
        let layout = pane_layout(width, self.wide_collapsed, self.narrow_open);
        let snapshot = self.snapshot();
        let item = |route: Route| {
            NavItem::new(pages::title(route, self.locale), route_icon(route))
                .badge(info_badge((pages::module(route).badge)(&snapshot)))
        };
        let entries = Route::PANE
            .iter()
            .map(|route| item(*route).entry())
            .collect();
        let selected = Route::of(&self.page).pane_index();
        let field = self.search_box(cx);
        let nav_search = self.nav_search.clone();
        NavigationView::new("control-center-nav", entries, Some(selected))
            .footer(vec![item(Route::Settings)])
            .compact(layout.compact)
            .overlay(layout.overlay)
            .locale(self.locale.words())
            .search(field, s.search_placeholder)
            .on_search_open(move |window, cx| {
                nav_search.update(cx, |input, cx| input.focus(window, cx));
            })
            .on_select(Self::act_with(cx, |this, index: usize, _, cx| {
                if let Some(route) = Route::from_pane_index(index) {
                    let page = crate::navigation::landing(route, this.transaction_running());
                    this.navigate(page, cx);
                }
            }))
            .on_toggle(Self::act_with(cx, move |this, compact: bool, _, cx| {
                this.set_pane_compact(compact, narrow, cx);
            }))
            .content(content)
            .into_any_element()
    }

    /// The heading today's Better Manager screens draw at their top, in the
    /// kit's Title and secondary Body. A screen with a component list gets
    /// the component search under it, at every width (audit M5).
    pub(crate) fn page_heading(
        &self,
        title: &'static str,
        subtitle: &'static str,
        show_search: bool,
        _compact: bool,
    ) -> AnyElement {
        let p = palette_for(self.dark);
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .child(label(title, TITLE, p.text_primary))
            .child(label(subtitle, BODY, p.text_secondary).whitespace_normal())
            .when(show_search, |view| {
                view.child(
                    div()
                        .w_full()
                        .max_w(px(360.0))
                        .pt(spacing::MD)
                        .child(TextBox::search("component-search", &self.search)),
                )
            })
            .into_any_element()
    }
}
