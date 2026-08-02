from pathlib import Path

path = Path("crates/monitor-gui/src/app.rs")
text = path.read_text()


def replace_once(old: str, new: str) -> None:
    global text
    if new in text:
        return
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected one match, found {count}: {old[:80]!r}")
    text = text.replace(old, new, 1)


replace_once(
    "    ActiveTheme, Disableable, Root, Selectable as _, Sizable, StyledExt,\n",
    "    ActiveTheme, Disableable, Icon, Root, Selectable as _, Sizable, StyledExt,\n",
)
replace_once(
    "use monitor_core::{Incident, MonitorStore, Sample};\n",
    "use gpui_platform::ApplicationExt;\nuse monitor_core::{Incident, MonitorStore, Sample};\n",
)
replace_once(
    """    const fn marker(self) -> &'static str {
        match self {
            Self::Overview => "◫",
            Self::Apps => "▦",
            Self::Processes => "≡",
            Self::Cpu => "◇",
            Self::Memory => "▤",
            Self::Gpu => "▰",
            Self::Npu => "◇",
            Self::Storage => "▱",
            Self::Network => "⌁",
            Self::Battery => "▥",
            Self::History => "↗",
            Self::Incidents => "!",
            Self::Diagnostics => "⊙",
            Self::Settings => "⚙",
        }
    }
""",
    """    const fn icon_path(self) -> &'static str {
        match self {
            Self::Overview => "icons/gauge.svg",
            Self::Apps => "icons/layout-grid.svg",
            Self::Processes => "icons/search.svg",
            Self::Cpu | Self::Gpu | Self::Npu => "icons/cpu.svg",
            Self::Memory => "icons/memory-stick.svg",
            Self::Storage => "icons/hard-drive.svg",
            Self::Network => "icons/network.svg",
            Self::Battery => "icons/battery.svg",
            Self::History | Self::Diagnostics => "icons/gauge.svg",
            Self::Incidents => "icons/triangle-alert.svg",
            Self::Settings => "icons/settings.svg",
        }
    }
""",
)
replace_once(
    """        Button::new(page.id())
            .ghost()
            .small()
            .w_full()
            .label(format!(
                "{}   {}",
                page.marker(),
                page.label(self.settings.locale)
            ))
""",
    """        Button::new(page.id())
            .ghost()
            .small()
            .w_full()
            .icon(Icon::default().path(page.icon_path()))
            .label(page.label(self.settings.locale))
""",
)
replace_once(
    """        Button::new(format!("compact-{}", page.id()))
            .ghost()
            .small()
            .flex_shrink_0()
            .label(format!(
                "{}  {}",
                page.marker(),
                page.label(self.settings.locale)
            ))
""",
    """        Button::new(format!("compact-{}", page.id()))
            .ghost()
            .small()
            .flex_shrink_0()
            .icon(Icon::default().path(page.icon_path()))
            .label(page.label(self.settings.locale))
""",
)
replace_once(
    "    gpui_platform::application().run(move |cx| {\n",
    "    gpui_platform::application()\n        .with_assets(gpui_component_assets::Assets)\n        .run(move |cx| {\n",
)

path.write_text(text)
