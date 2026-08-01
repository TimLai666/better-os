from pathlib import Path
import re


def replace_once(source: str, old: str, new: str, label: str) -> str:
    if new in source:
        return source
    if old not in source:
        raise SystemExit(f"missing anchor: {label}")
    return source.replace(old, new, 1)


app_table_path = Path("crates/monitor-gui/src/app_table.rs")
source = app_table_path.read_text()
source = replace_once(
    source,
    "use gpui::*;\n",
    "use better_ui::Locale;\nuse gpui::*;\n",
    "Locale import",
)
source = replace_once(
    source,
    "    app::MonitorWindow,\n    linux::{self, AppGroup},",
    "    app::MonitorWindow,\n    i18n::{\n        CopyKey, app_information_title, app_process_count, end_application_title,\n        force_stop_application_title, text,\n    },\n    linux::{self, AppGroup},",
    "i18n imports",
)

old_title = '''    const fn title(self) -> &'static str {
        match self {
            Self::Name => "App",
            Self::Memory => "Memory",
            Self::Cpu => "CPU %",
            Self::ReadSpeed => "Read/s",
            Self::ReadTotal => "Read total",
            Self::WriteSpeed => "Write/s",
            Self::WriteTotal => "Write total",
            Self::Gpu => "GPU %",
            Self::GpuMemory => "GPU memory",
            Self::Encoder => "Encoder %",
            Self::Decoder => "Decoder %",
            Self::Swap => "Swap",
            Self::CombinedMemory => "Memory + swap",
            Self::Actions => "Actions",
        }
    }
'''
new_title = '''    fn title(self, locale: Locale) -> &'static str {
        text(
            locale,
            match self {
                Self::Name => CopyKey::App,
                Self::Memory => CopyKey::Memory,
                Self::Cpu => CopyKey::CpuPercent,
                Self::ReadSpeed => CopyKey::ReadPerSecond,
                Self::ReadTotal => CopyKey::ReadTotal,
                Self::WriteSpeed => CopyKey::WritePerSecond,
                Self::WriteTotal => CopyKey::WriteTotal,
                Self::Gpu => CopyKey::GpuPercent,
                Self::GpuMemory => CopyKey::GpuMemory,
                Self::Encoder => CopyKey::EncoderPercent,
                Self::Decoder => CopyKey::DecoderPercent,
                Self::Swap => CopyKey::Swap,
                Self::CombinedMemory => CopyKey::CombinedMemory,
                Self::Actions => CopyKey::Actions,
            },
        )
    }
'''
source = replace_once(source, old_title, new_title, "localized App column titles")
source = source.replace(
    "Column::new(kind.id(), kind.title()).width(kind.width())",
    "Column::new(kind.id(), kind.title(self.settings.locale)).width(kind.width())",
    1,
)
source = source.replace(
    '''            AppColumn::Name => {
                format!("{} · {} processes", group.display_name, group.process_count)
            }''',
    '''            AppColumn::Name => format!(
                "{} · {}",
                group.display_name,
                app_process_count(self.settings.locale, group.process_count)
            ),''',
    1,
)
source = source.replace(
    '                "N/A".to_string()\n',
    '                text(self.settings.locale, CopyKey::Unavailable).to_string()\n',
    1,
)
source = source.replace(
    '            AppColumn::Actions => "Application actions".to_string(),',
    '            AppColumn::Actions => text(self.settings.locale, CopyKey::ApplicationActions).to_string(),',
    1,
)

source = replace_once(
    source,
    "        let info_unit = self.settings.unit_base;\n",
    "        let info_unit = self.settings.unit_base;\n        let locale = self.settings.locale;\n",
    "render action locale",
)
source = source.replace(
    ".label(\"Info\")",
    ".label(text(locale, CopyKey::Info))",
    1,
)
source = source.replace(
    "                                info_unit,\n                                row_ix,",
    "                                info_unit,\n                                locale,\n                                row_ix,",
    1,
)
source = source.replace(
    ".label(\"End\")",
    ".label(text(locale, CopyKey::End))",
    1,
)
source = source.replace(
    'title: format!("End {}?", term_group.display_name),',
    'title: end_application_title(locale, &term_group.display_name),',
    1,
)
source = source.replace(
    'description: "All processes in this application group will receive a graceful termination signal.",',
    'description: text(locale, CopyKey::GracefulApplicationDescription),',
    1,
)
source = source.replace(
    'confirm_label: "End Application",',
    'confirm_label: text(locale, CopyKey::EndApplication),',
    1,
)
source = source.replace(
    "                        row_ix,\n                    )\n",
    "                        row_ix,\n                        locale,\n                    )\n",
    1,
)

source = source.replace(
    "fn app_information(group: &AppGroup, unit_base: UnitBase) -> String {",
    "fn app_information(group: &AppGroup, unit_base: UnitBase, locale: Locale) -> String {",
    1,
)
source = source.replace(
    '''        "Identity: {}\\nGrouped processes: {}\\nGrouping evidence: {}\\nPIDs: {}\\nCPU: {:.1}%\\nMemory: {}\\nSwap: {}\\nRead: {} total, {}/s\\nWritten: {} total, {}/s",
        group.id,
        group.process_count,
        group.grouping_reason,''',
    '''        "{}: {}\\n{}: {}\\n{}: {}\\n{}: {}\\n{}: {:.1}%\\n{}: {}\\n{}: {}\\n{}: {} total, {}/s\\n{}: {} total, {}/s",
        text(locale, CopyKey::Identity),
        group.id,
        text(locale, CopyKey::GroupedProcesses),
        group.process_count,
        text(locale, CopyKey::GroupingEvidence),
        group.grouping_reason,
        text(locale, CopyKey::Pids),''',
    1,
)
source = source.replace(
    '''            .join(", "),
        group.cpu_usage,
        linux::format_bytes(group.memory, unit_base),
        linux::format_bytes(group.swap, unit_base),
        linux::format_bytes(group.read_total, unit_base),
        linux::format_bytes(group.read_speed, unit_base),
        linux::format_bytes(group.write_total, unit_base),
        linux::format_bytes(group.write_speed, unit_base),''',
    '''            .join(", "),
        text(locale, CopyKey::Cpu),
        group.cpu_usage,
        text(locale, CopyKey::Memory),
        linux::format_bytes(group.memory, unit_base),
        text(locale, CopyKey::Swap),
        linux::format_bytes(group.swap, unit_base),
        text(locale, CopyKey::Read),
        linux::format_bytes(group.read_total, unit_base),
        linux::format_bytes(group.read_speed, unit_base),
        text(locale, CopyKey::Written),
        linux::format_bytes(group.write_total, unit_base),
        linux::format_bytes(group.write_speed, unit_base),''',
    1,
)
source = source.replace(
    "    unit_base: UnitBase,\n    row_ix: usize,",
    "    unit_base: UnitBase,\n    locale: Locale,\n    row_ix: usize,",
    1,
)
source = source.replace(
    '    let title = format!("{} Information", group.display_name);\n    let description = app_information(&group, unit_base);',
    '    let title = app_information_title(locale, &group.display_name);\n    let description = app_information(&group, unit_base, locale);',
    1,
)
source = source.replace(
    '.label("Close")',
    '.label(text(locale, CopyKey::Close))',
    1,
)
source = source.replace(
    '.label("Cancel")',
    '.label(text(locale, CopyKey::Cancel))',
    1,
)

source = source.replace(
    "    row_ix: usize,\n) -> PopupMenu {",
    "    row_ix: usize,\n    locale: Locale,\n) -> PopupMenu {",
    1,
)
source = source.replace(
    'PopupMenuItem::new("Force stop")',
    'PopupMenuItem::new(text(locale, CopyKey::ForceStop))',
    1,
)
source = source.replace(
    'title: format!("Force stop {}?", force_group.display_name),',
    'title: force_stop_application_title(locale, &force_group.display_name),',
    1,
)
source = source.replace(
    'description: "Every process in this application group will stop immediately without cleanup.",',
    'description: text(locale, CopyKey::ForceApplicationDescription),',
    1,
)
source = source.replace(
    'confirm_label: "Force stop",',
    'confirm_label: text(locale, CopyKey::ForceStop),',
    1,
)
source = source.replace(
    'PopupMenuItem::new("Pause")',
    'PopupMenuItem::new(text(locale, CopyKey::Pause))',
    1,
)
source = source.replace(
    'PopupMenuItem::new("Resume")',
    'PopupMenuItem::new(text(locale, CopyKey::Resume))',
    1,
)

source = replace_once(
    source,
    "        let info_unit = self.settings.unit_base;\n        let end_monitor",
    "        let info_unit = self.settings.unit_base;\n        let locale = self.settings.locale;\n        let end_monitor",
    "context menu locale",
)
source = source.replace(
    'PopupMenuItem::new("Application information")',
    'PopupMenuItem::new(text(locale, CopyKey::ApplicationInformation))',
    1,
)
# Context-menu information call is the second information dialog call.
context_marker = "open_app_information_dialog("
first = source.index(context_marker)
second = source.index(context_marker, first + 1)
call_end = source.index("row_ix,", second) + len("row_ix,")
source = source[:call_end] + "\n                            locale," + source[call_end:]
source = source.replace(
    'PopupMenuItem::new("End application")',
    'PopupMenuItem::new(text(locale, CopyKey::EndApplication))',
    1,
)
source = source.replace(
    'title: format!("End {}?", end_group.display_name),',
    'title: end_application_title(locale, &end_group.display_name),',
    1,
)
# Replace the second graceful description and confirm label if still present.
source = source.replace(
    'description: "All processes in this application group will receive a graceful termination signal.",',
    'description: text(locale, CopyKey::GracefulApplicationDescription),',
    1,
)
source = source.replace(
    'confirm_label: "End Application",',
    'confirm_label: text(locale, CopyKey::EndApplication),',
    1,
)
source = source.replace(
    "        app_action_menu(menu, action_monitor, group, row_ix)",
    "        app_action_menu(menu, action_monitor, group, row_ix, locale)",
    1,
)

app_table_path.write_text(source)

app_path = Path("crates/monitor-gui/src/app.rs")
app = app_path.read_text()
app = replace_once(
    app,
    "    linux::{\n",
    "    i18n::{CopyKey, text},\n    linux::{\n",
    "app i18n imports",
)
app = app.replace(
    '.placeholder("Search apps and processes…")',
    '.placeholder(text(settings.locale, CopyKey::SearchPlaceholder))',
    1,
)
app_path.write_text(app)

parity_path = Path("crates/monitor-gui/src/parity.rs")
parity = parity_path.read_text()
old_listener = '''                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.locale = this.settings.locale.next();
                                        this.persist_settings();
                                        cx.notify();
                                    })),'''
new_listener = '''                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.settings.locale = this.settings.locale.next();
                                        let placeholder = text(
                                            this.settings.locale,
                                            CopyKey::SearchPlaceholder,
                                        );
                                        this.search_input.update(cx, |input, cx| {
                                            input.set_placeholder(placeholder, window, cx);
                                        });
                                        this.sync_table_settings(cx);
                                    })),'''
parity = replace_once(parity, old_listener, new_listener, "runtime locale refresh")
parity_path.write_text(parity)

checklist_path = Path("docs/better-monitor-resources-v1.10.2-parity.md")
checklist = checklist_path.read_text()
checklist = re.sub(
    r"^\| Pseudo-long locale tests \|.*$",
    "| Pseudo-long locale tests | 🟨 | The scoped Apps catalog is expanded to at least 200% in tests; Processes and remaining pages still need catalog coverage. |",
    checklist,
    flags=re.MULTILINE,
)
checklist = re.sub(
    r"^\| Runtime switching \|.*$",
    "| Runtime switching | 🟨 | Shell, Apps table headers/actions/dialogs, and the shared search placeholder switch immediately; Processes and remaining page copy still need catalog coverage. |",
    checklist,
    flags=re.MULTILINE,
)
checklist_path.write_text(checklist)
