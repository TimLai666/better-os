from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    source = file.read_text()
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}: {old[:100]!r}")
    file.write_text(source.replace(old, new, 1))


UI = "crates/better-ui/src/lib.rs"
APP = "crates/monitor-gui/src/app.rs"
PARITY = "crates/monitor-gui/src/parity.rs"
DOC = "docs/better-monitor-resources-v1.10.2-parity.md"

replace_once(
    UI,
    "use gpui::{Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled, div};",
    "use gpui::{Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled, div, px};",
)

replace_once(
    UI,
    """}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusCard {""",
    """}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportStateKind {
    Success,
    Info,
    Unavailable,
    PermissionDenied,
    Stale,
    CollectorError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupportState {
    pub kind: SupportStateKind,
    pub title: String,
    pub detail: String,
}

impl SupportState {
    pub fn new(
        kind: SupportStateKind,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct SupportStatePalette {
    pub border: Hsla,
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub info: Hsla,
    pub radius: Pixels,
}

impl SupportStatePalette {
    fn accent(self, kind: SupportStateKind) -> Hsla {
        match kind {
            SupportStateKind::Success => self.success,
            SupportStateKind::Info => self.info,
            SupportStateKind::Unavailable => self.muted_foreground,
            SupportStateKind::PermissionDenied | SupportStateKind::CollectorError => self.danger,
            SupportStateKind::Stale => self.warning,
        }
    }
}

pub fn support_state_panel(
    state: &SupportState,
    palette: SupportStatePalette,
) -> impl IntoElement {
    let accent = palette.accent(state.kind);
    h_flex()
        .items_center()
        .gap_3()
        .min_w_0()
        .rounded(palette.radius)
        .border_1()
        .border_color(palette.border)
        .bg(palette.background)
        .p_3()
        .child(div().size_3().flex_shrink_0().rounded(px(99.0)).bg(accent))
        .child(
            v_flex()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .font_bold()
                        .text_color(accent)
                        .child(state.title.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(palette.muted_foreground)
                        .child(state.detail.clone()),
                ),
        )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusCard {""",
)

replace_once(
    UI,
    """    fn locale_values_round_trip_and_cycle() {
        for locale in [Locale::System, Locale::EnUs, Locale::ZhTw] {
            assert_eq!(Locale::parse(locale.config_value()), locale);
        }
        assert_eq!(Locale::System.next(), Locale::EnUs);
        assert_eq!(Locale::EnUs.next(), Locale::ZhTw);
        assert_eq!(Locale::ZhTw.next(), Locale::System);
    }
""",
    """    fn locale_values_round_trip_and_cycle() {
        for locale in [Locale::System, Locale::EnUs, Locale::ZhTw] {
            assert_eq!(Locale::parse(locale.config_value()), locale);
        }
        assert_eq!(Locale::System.next(), Locale::EnUs);
        assert_eq!(Locale::EnUs.next(), Locale::ZhTw);
        assert_eq!(Locale::ZhTw.next(), Locale::System);
    }

    #[test]
    fn support_state_keeps_semantics_separate_from_copy() {
        let state = SupportState::new(
            SupportStateKind::PermissionDenied,
            "Permission required",
            "Linux rejected the operation",
        );
        assert_eq!(state.kind, SupportStateKind::PermissionDenied);
        assert_eq!(state.title, "Permission required");
        assert_eq!(state.detail, "Linux rejected the operation");
    }
""",
)

replace_once(
    APP,
    "use better_ui::{Locale, page_heading};",
    "use better_ui::{\n    Locale, SupportState, SupportStateKind, SupportStatePalette, page_heading,\n    support_state_panel,\n};",
)
replace_once(APP, "    last_action: Option<String>,", "    last_action: Option<SupportState>,")

replace_once(
    PARITY,
    """    fn unavailable_page(
        &self,
        title: &'static str,
        description: &'static str,
        cx: &Context<Self>,
    ) -> Div {
        v_flex()
            .items_center()
            .justify_center()
            .gap_2()
            .min_h(px(420.0))
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(div().text_lg().font_bold().child(title))
            .child(
                div()
                    .max_w(px(620.0))
                    .text_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
    }

    fn action_result_banner(&self, message: String, cx: &Context<Self>) -> Div {
        h_flex()
            .items_center()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .p_3()
            .child(div().size_2().rounded(px(99.0)).bg(cx.theme().blue))
            .child(div().text_sm().child(message))
    }
""",
    """    fn support_state_palette(&self, cx: &Context<Self>) -> SupportStatePalette {
        SupportStatePalette {
            border: cx.theme().border,
            background: cx.theme().background,
            foreground: cx.theme().foreground,
            muted_foreground: cx.theme().muted_foreground,
            success: cx.theme().green,
            warning: cx.theme().yellow,
            danger: cx.theme().red,
            info: cx.theme().blue,
            radius: cx.theme().radius,
        }
    }

    fn unavailable_page(
        &self,
        title: &'static str,
        description: &'static str,
        cx: &Context<Self>,
    ) -> Div {
        let state = SupportState::new(SupportStateKind::Unavailable, title, description);
        v_flex()
            .items_center()
            .justify_center()
            .min_h(px(420.0))
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .p_5()
            .child(
                div()
                    .w_full()
                    .max_w(px(680.0))
                    .child(support_state_panel(&state, self.support_state_palette(cx))),
            )
    }

    fn action_result_banner(
        &self,
        state: SupportState,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        support_state_panel(&state, self.support_state_palette(cx))
    }
""",
)

replace_once(
    PARITY,
    """    pub(crate) fn signal_pids(&mut self, pids: &[Pid], signal: Signal) {
        let mut succeeded = 0;
        let mut stale = 0;
        let mut denied_or_unsupported = 0;
        for pid in pids {
            let Some(process) = self.system.process(*pid) else {
                stale += 1;
                continue;
            };
            match process.kill_with(signal) {
                Some(true) => succeeded += 1,
                _ => denied_or_unsupported += 1,
            }
        }
        self.last_action = Some(format!(
            "Signal {signal:?}: {succeeded} succeeded, {stale} stale, {denied_or_unsupported} denied or unsupported"
        ));
    }

    fn persist_settings(&mut self) {
        self.last_action = match self.settings.save() {
            Ok(()) => Some("Settings saved".to_string()),
            Err(error) => Some(format!("Could not save settings: {error}")),
        };
    }
""",
    """    pub(crate) fn signal_pids(&mut self, pids: &[Pid], signal: Signal) {
        let mut succeeded = 0;
        let mut stale = 0;
        let mut denied = 0;
        let mut unsupported = 0;
        for pid in pids {
            let Some(process) = self.system.process(*pid) else {
                stale += 1;
                continue;
            };
            match process.kill_with(signal) {
                Some(true) => succeeded += 1,
                Some(false) => denied += 1,
                None => unsupported += 1,
            }
        }

        let kind = if denied > 0 {
            SupportStateKind::PermissionDenied
        } else if unsupported > 0 {
            SupportStateKind::Unavailable
        } else if stale > 0 {
            SupportStateKind::Stale
        } else {
            SupportStateKind::Success
        };
        let locale = self.settings.locale.resolved();
        let title = match (locale, kind) {
            (Locale::ZhTw, SupportStateKind::PermissionDenied) => "需要額外權限",
            (Locale::ZhTw, SupportStateKind::Unavailable) => "此操作無法使用",
            (Locale::ZhTw, SupportStateKind::Stale) => "程序已經結束",
            (Locale::ZhTw, _) => "程序操作完成",
            (_, SupportStateKind::PermissionDenied) => "Permission required",
            (_, SupportStateKind::Unavailable) => "Action unavailable",
            (_, SupportStateKind::Stale) => "Process already ended",
            (_, _) => "Process action finished",
        };
        let detail = match locale {
            Locale::ZhTw => format!(
                "{signal:?}：成功 {succeeded} 個、已結束 {stale} 個、權限不足 {denied} 個、不支援 {unsupported} 個"
            ),
            _ => format!(
                "Signal {signal:?}: {succeeded} succeeded, {stale} stale, {denied} denied, {unsupported} unsupported"
            ),
        };
        self.last_action = Some(SupportState::new(kind, title, detail));
    }

    fn persist_settings(&mut self) {
        let locale = self.settings.locale.resolved();
        self.last_action = match self.settings.save() {
            Ok(()) => Some(SupportState::new(
                SupportStateKind::Success,
                match locale {
                    Locale::ZhTw => "設定已儲存",
                    _ => "Settings saved",
                },
                match locale {
                    Locale::ZhTw => "新的偏好設定已寫入使用者設定檔。",
                    _ => "The updated preferences were written to the user configuration.",
                },
            )),
            Err(error) => Some(SupportState::new(
                SupportStateKind::CollectorError,
                match locale {
                    Locale::ZhTw => "無法儲存設定",
                    _ => "Could not save settings",
                },
                error.to_string(),
            )),
        };
    }
""",
)

replace_once(
    DOC,
    "| Permission state | 🟨 | Process control returns Linux errors. Dedicated permission UI and narrow Polkit helper flow are missing. |",
    "| Permission state | 🟨 | Typed permission-denied panels are used for process actions. A narrow Polkit helper flow is still missing. |",
)
replace_once(
    DOC,
    "| Stale-process state | 🟨 | Selections are pruned and action results count stale PIDs separately; a dedicated stale-state visual remains missing. |",
    "| Stale-process state | ✅ | Selections are pruned and typed stale feedback is rendered separately when a PID has already disappeared. |",
)
replace_once(
    DOC,
    "| Empty / unsupported / error state | 🟨 | Unsupported device page helper exists. Dedicated unknown, stale, permission-denied, and collector-error visuals are incomplete. |",
    "| Empty / unsupported / error state | 🟨 | A shared typed panel now covers unavailable, permission-denied, stale, collector-error, success, and info states. Collector-specific adoption and unknown-state copy remain incomplete. |",
)
replace_once(
    DOC,
    "| Toast / result banner | ✅ | Action result banner exists. |",
    "| Toast / result banner | ✅ | Action feedback uses the shared typed support-state panel instead of inferring semantics from message text. |",
)
replace_once(
    DOC,
    "2. Build the remaining shared parity interaction components: context menu, split action, and support-state panel.",
    "2. Build the remaining shared parity interaction components: context menu and split action.",
)
