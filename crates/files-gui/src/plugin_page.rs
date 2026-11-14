//! A plugin's own page in the main pane (folio's plugin `view`, at
//! `plugin:<id>`): every item the plugin offers, in its order, with icon,
//! name and subtitle; the selected item's details beside them; Enter, a
//! double click or Open opens one. It follows the polls, so items come and
//! go while it is open. Nothing on it can be copied, renamed, pinned or
//! previewed, so none of that is offered.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{Icon, IconName};

use better_ui::fluent::{self, BODY, BODY_STRONG, CAPTION};

use crate::app::FilesApp;
use crate::plugins::{Item, Plugin};

/// The page that is open, and where the window stood when it opened: moving
/// anywhere else closes it.
#[derive(Clone, Debug)]
pub(crate) struct PluginPage {
    pub plugin_id: String,
    pub at: files_core::Location,
    pub selected: Option<usize>,
}

/// The icon a plugin's item names, among the kit's.
pub(crate) fn item_icon(icon: Option<&str>) -> IconName {
    match icon {
        Some("peer" | "tailnet") => IconName::Globe,
        Some("drive") => IconName::HardDrive,
        Some("folder" | "documents" | "downloads" | "pictures" | "music" | "videos") => {
            IconName::Folder
        }
        _ => IconName::File,
    }
}

impl FilesApp {
    /// The plugin and its current items for the open page, if any.
    pub(crate) fn plugin_page_items(&self) -> Option<(Plugin, Vec<Item>)> {
        let page = self.plugin_view.as_ref()?;
        self.plugins
            .snapshot()
            .into_iter()
            .find(|(plugin, _)| plugin.id == page.plugin_id && plugin.manifest.view.is_some())
            .map(|(plugin, poll)| (plugin, poll.map(|poll| poll.items).unwrap_or_default()))
    }

    /// The open page's title, for the path bar.
    pub(crate) fn plugin_page_title(&self) -> Option<String> {
        let (plugin, _) = self.plugin_page_items()?;
        let view = plugin.manifest.view.as_ref()?;
        Some(plugin.translate(&view.title))
    }

    /// Opens `plugin:<id>`. False when no plugin by that id has a view.
    pub(crate) fn open_plugin_view(&mut self, id: &str) -> bool {
        let known = self
            .plugins
            .snapshot()
            .iter()
            .any(|(plugin, _)| plugin.id == id && plugin.manifest.view.is_some());
        if known {
            self.plugin_view = Some(PluginPage {
                plugin_id: id.to_string(),
                at: self.session.location().clone(),
                selected: None,
            });
        }
        known
    }

    /// Opens one of a plugin's items (from its row or its page) and puts it
    /// first among the ones its section remembers.
    pub(crate) fn open_plugin_item(&mut self, plugin: &Plugin, item: &Item) {
        let opened = self.plugin_opened.entry(plugin.id.clone()).or_default();
        crate::plugins::remember_opened(opened, &item.id);
        if let Some(path) = crate::plugins::opened_path() {
            let _ = crate::plugins::save_opened(&path, &self.plugin_opened);
        }
        match &item.open {
            Some(crate::plugins::Open::Path { path }) => {
                let path = crate::plugins::expand(path, self.plugins.home());
                if let Ok(location) = files_core::Location::local(path) {
                    self.plugin_view = None;
                    self.session.navigate_to(location);
                }
            }
            Some(crate::plugins::Open::Mount { mount }) => {
                self.plugin_view = None;
                self.session.mount_server(mount.clone(), None);
            }
            Some(crate::plugins::Open::Run { run }) => {
                self.plugins.run(
                    plugin.clone(),
                    run.clone(),
                    Vec::new(),
                    Some(item.clone()),
                    false,
                );
            }
            None => {}
        }
    }

    /// Up, Down, Enter and Escape on the open page. True when the key was
    /// the page's.
    pub(crate) fn plugin_page_key(&mut self, key: &str) -> bool {
        let Some((plugin, items)) = self.plugin_page_items() else {
            return false;
        };
        let Some(page) = self.plugin_view.as_mut() else {
            return false;
        };
        match key {
            "down" if !items.is_empty() => {
                page.selected = Some(page.selected.map_or(0, |at| (at + 1).min(items.len() - 1)));
            }
            "up" if !items.is_empty() => {
                page.selected = Some(page.selected.map_or(0, |at| at.saturating_sub(1)));
            }
            "enter" => {
                if let Some(item) = page.selected.and_then(|at| items.get(at)).cloned() {
                    self.open_plugin_item(&plugin, &item);
                }
            }
            "escape" => self.plugin_view = None,
            _ => return false,
        }
        true
    }

    /// The page itself, in place of the folder's content.
    pub(crate) fn plugin_page(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (plugin, items) = self.plugin_page_items()?;
        let view = plugin.manifest.view.clone()?;
        let p = fluent::palette(cx);
        let palette = *p;
        let selected = self
            .plugin_view
            .as_ref()
            .and_then(|page| page.selected)
            .filter(|at| *at < items.len());
        let this = cx.entity().downgrade();
        let rows = items.iter().enumerate().map(|(index, item)| {
            let subtitle = item
                .subtitle
                .as_ref()
                .map(|subtitle| subtitle.render(&plugin.catalog))
                .unwrap_or_default();
            let chosen = selected == Some(index);
            let this = this.clone();
            let plugin = plugin.clone();
            let item = item.clone();
            div()
                .id(SharedString::from(format!("plugin-item-{index}")))
                .w_full()
                .h(px(36.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .rounded(fluent::radius::SM)
                .when(chosen, |row| row.bg(palette.subtle_fill))
                .hover(move |row| row.bg(palette.subtle_fill))
                .child(
                    Icon::new(item_icon(item.icon.as_deref()))
                        .size(px(16.0))
                        .text_color(palette.text_secondary),
                )
                .child(fluent::label(item.name.clone(), BODY, palette.text_primary).flex_1())
                .child(fluent::label(subtitle, CAPTION, palette.text_secondary))
                .on_click(move |event, _window, cx| {
                    let double = event.click_count() >= 2;
                    let _ = this.update(cx, |app, cx| {
                        if let Some(page) = app.plugin_view.as_mut() {
                            page.selected = Some(index);
                        }
                        if double {
                            app.open_plugin_item(&plugin, &item);
                        }
                        cx.notify();
                    });
                })
        });
        let details = selected.and_then(|at| items.get(at)).map(|item| {
            let mut column = div()
                .w(px(300.0))
                .flex_none()
                .h_full()
                .p(px(16.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .border_l_1()
                .border_color(palette.divider)
                .child(fluent::label(
                    item.name.clone(),
                    BODY_STRONG,
                    palette.text_primary,
                ));
            if let Some(subtitle) = &item.subtitle {
                column = column.child(fluent::label(
                    subtitle.render(&plugin.catalog),
                    CAPTION,
                    palette.text_secondary,
                ));
            }
            for (label, value) in &item.details {
                column = column.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(fluent::label(
                            plugin.translate(label),
                            CAPTION,
                            palette.text_secondary,
                        ))
                        .child(fluent::label(value.clone(), BODY, palette.text_primary)),
                );
            }
            column
        });
        let list = if items.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(fluent::label(
                    view.empty
                        .as_deref()
                        .map(|empty| plugin.translate(empty))
                        .unwrap_or_default(),
                    BODY,
                    palette.text_secondary,
                ))
                .into_any_element()
        } else {
            div()
                .id("plugin-page-items")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .p(px(8.0))
                .flex()
                .flex_col()
                .children(rows)
                .into_any_element()
        };
        Some(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .child(list)
                .children(details)
                .into_any_element(),
        )
    }
}
