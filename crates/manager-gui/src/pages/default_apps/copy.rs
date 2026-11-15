//! The words Default apps adds to today's Defaults copy (spec section 8).
//!
//! Everything the Defaults screens said before is still said, from
//! `i18n::Copy`; this holds only what the control center's page needs on top:
//! its description, the summary line, the overflow menu's Restore, and the
//! Home card.

use crate::i18n::Locale;

pub(crate) struct PageCopy {
    /// Under the title.
    pub(crate) description: &'static str,
    /// `{are_default}`, `{can_change}`, `{changed}`.
    pub(crate) summary: &'static str,
    /// The overflow menu's restore for one component.
    pub(crate) restore: &'static str,
    /// The overflow button's tooltip; `{component}`.
    pub(crate) more_actions: &'static str,
    /// The review screen's bottom summary.
    pub(crate) review_summary: &'static str,
    /// A component's row on the review screen; `{n}`.
    pub(crate) review_changes_one: &'static str,
    pub(crate) review_changes: &'static str,
    /// Home: a default changed elsewhere.
    pub(crate) home_changed_header: &'static str,
    /// `{n}`.
    pub(crate) home_changed_description_one: &'static str,
    pub(crate) home_changed_description: &'static str,
    pub(crate) view: &'static str,
}

static EN: PageCopy = PageCopy {
    description: "Make Better OS components the default for files, links, and features, and put things back any time.",
    summary: "{are_default} are defaults · {can_change} can change · {changed} changed elsewhere",
    restore: "Restore",
    more_actions: "More actions for {component}",
    review_summary: "Summary",
    review_changes_one: "1 setting affected",
    review_changes: "{n} settings affected",
    home_changed_header: "A default was changed by another app",
    home_changed_description_one: "1 setting no longer matches what Better OS set.",
    home_changed_description: "{n} settings no longer match what Better OS set.",
    view: "View",
};

static ZH_TW: PageCopy = PageCopy {
    description: "讓 Better OS 元件成為開啟檔案、連結與功能的預設程式，並可隨時恢復。",
    summary: "{are_default} 個已是預設 · {can_change} 個可以變更 · {changed} 個被其他程式變更",
    restore: "恢復",
    more_actions: "{component} 的更多動作",
    review_summary: "摘要",
    review_changes_one: "影響 1 項設定",
    review_changes: "影響 {n} 項設定",
    home_changed_header: "有預設值被其他程式變更",
    home_changed_description_one: "1 項設定和 Better OS 套用的不同。",
    home_changed_description: "{n} 項設定和 Better OS 套用的不同。",
    view: "查看",
};

pub(crate) fn page_copy(locale: Locale) -> &'static PageCopy {
    match locale.resolved() {
        Locale::ZhTw => &ZH_TW,
        Locale::EnUs | Locale::System => &EN,
    }
}
