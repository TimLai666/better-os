use better_ui::Locale;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CopyKey {
    App,
    Memory,
    CpuPercent,
    ReadPerSecond,
    ReadTotal,
    WritePerSecond,
    WriteTotal,
    GpuPercent,
    GpuMemory,
    EncoderPercent,
    DecoderPercent,
    Swap,
    CombinedMemory,
    Actions,
    ApplicationActions,
    Info,
    End,
    ForceStop,
    Pause,
    Resume,
    ApplicationInformation,
    EndApplication,
    Close,
    Cancel,
    Unavailable,
    Identity,
    GroupedProcesses,
    GroupingEvidence,
    Pids,
    Cpu,
    Read,
    Written,
    GracefulApplicationDescription,
    ForceApplicationDescription,
    SearchPlaceholder,
}

impl CopyKey {
    pub(crate) const ALL: [Self; 35] = [
        Self::App,
        Self::Memory,
        Self::CpuPercent,
        Self::ReadPerSecond,
        Self::ReadTotal,
        Self::WritePerSecond,
        Self::WriteTotal,
        Self::GpuPercent,
        Self::GpuMemory,
        Self::EncoderPercent,
        Self::DecoderPercent,
        Self::Swap,
        Self::CombinedMemory,
        Self::Actions,
        Self::ApplicationActions,
        Self::Info,
        Self::End,
        Self::ForceStop,
        Self::Pause,
        Self::Resume,
        Self::ApplicationInformation,
        Self::EndApplication,
        Self::Close,
        Self::Cancel,
        Self::Unavailable,
        Self::Identity,
        Self::GroupedProcesses,
        Self::GroupingEvidence,
        Self::Pids,
        Self::Cpu,
        Self::Read,
        Self::Written,
        Self::GracefulApplicationDescription,
        Self::ForceApplicationDescription,
        Self::SearchPlaceholder,
    ];
}

pub(crate) fn text(locale: Locale, key: CopyKey) -> &'static str {
    match locale.resolved() {
        Locale::ZhTw => match key {
            CopyKey::App => "應用程式",
            CopyKey::Memory => "記憶體",
            CopyKey::CpuPercent => "CPU %",
            CopyKey::ReadPerSecond => "讀取／秒",
            CopyKey::ReadTotal => "讀取總量",
            CopyKey::WritePerSecond => "寫入／秒",
            CopyKey::WriteTotal => "寫入總量",
            CopyKey::GpuPercent => "GPU %",
            CopyKey::GpuMemory => "GPU 記憶體",
            CopyKey::EncoderPercent => "編碼器 %",
            CopyKey::DecoderPercent => "解碼器 %",
            CopyKey::Swap => "交換空間",
            CopyKey::CombinedMemory => "記憶體＋交換空間",
            CopyKey::Actions => "操作",
            CopyKey::ApplicationActions => "應用程式操作",
            CopyKey::Info => "資訊",
            CopyKey::End => "結束",
            CopyKey::ForceStop => "強制停止",
            CopyKey::Pause => "暫停",
            CopyKey::Resume => "繼續",
            CopyKey::ApplicationInformation => "應用程式資訊",
            CopyKey::EndApplication => "結束應用程式",
            CopyKey::Close => "關閉",
            CopyKey::Cancel => "取消",
            CopyKey::Unavailable => "無法使用",
            CopyKey::Identity => "識別資訊",
            CopyKey::GroupedProcesses => "包含的程序",
            CopyKey::GroupingEvidence => "分組依據",
            CopyKey::Pids => "PID",
            CopyKey::Cpu => "CPU",
            CopyKey::Read => "讀取",
            CopyKey::Written => "寫入",
            CopyKey::GracefulApplicationDescription => {
                "此應用程式群組內的所有程序都會收到正常結束訊號。"
            }
            CopyKey::ForceApplicationDescription => {
                "此應用程式群組內的所有程序都會立即停止，不會進行清理。"
            }
            CopyKey::SearchPlaceholder => "搜尋應用程式與程序…",
        },
        _ => match key {
            CopyKey::App => "App",
            CopyKey::Memory => "Memory",
            CopyKey::CpuPercent => "CPU %",
            CopyKey::ReadPerSecond => "Read/s",
            CopyKey::ReadTotal => "Read total",
            CopyKey::WritePerSecond => "Write/s",
            CopyKey::WriteTotal => "Write total",
            CopyKey::GpuPercent => "GPU %",
            CopyKey::GpuMemory => "GPU memory",
            CopyKey::EncoderPercent => "Encoder %",
            CopyKey::DecoderPercent => "Decoder %",
            CopyKey::Swap => "Swap",
            CopyKey::CombinedMemory => "Memory + swap",
            CopyKey::Actions => "Actions",
            CopyKey::ApplicationActions => "Application actions",
            CopyKey::Info => "Info",
            CopyKey::End => "End",
            CopyKey::ForceStop => "Force stop",
            CopyKey::Pause => "Pause",
            CopyKey::Resume => "Resume",
            CopyKey::ApplicationInformation => "Application information",
            CopyKey::EndApplication => "End Application",
            CopyKey::Close => "Close",
            CopyKey::Cancel => "Cancel",
            CopyKey::Unavailable => "N/A",
            CopyKey::Identity => "Identity",
            CopyKey::GroupedProcesses => "Grouped processes",
            CopyKey::GroupingEvidence => "Grouping evidence",
            CopyKey::Pids => "PIDs",
            CopyKey::Cpu => "CPU",
            CopyKey::Read => "Read",
            CopyKey::Written => "Written",
            CopyKey::GracefulApplicationDescription => {
                "All processes in this application group will receive a graceful termination signal."
            }
            CopyKey::ForceApplicationDescription => {
                "Every process in this application group will stop immediately without cleanup."
            }
            CopyKey::SearchPlaceholder => "Search apps and processes…",
        },
    }
}

pub(crate) fn app_process_count(locale: Locale, count: usize) -> String {
    match locale.resolved() {
        Locale::ZhTw => format!("{count} 個程序"),
        _ => format!("{count} processes"),
    }
}

pub(crate) fn app_information_title(locale: Locale, name: &str) -> String {
    match locale.resolved() {
        Locale::ZhTw => format!("{name} 資訊"),
        _ => format!("{name} Information"),
    }
}

pub(crate) fn end_application_title(locale: Locale, name: &str) -> String {
    match locale.resolved() {
        Locale::ZhTw => format!("要結束 {name} 嗎？"),
        _ => format!("End {name}?"),
    }
}

pub(crate) fn force_stop_application_title(locale: Locale, name: &str) -> String {
    match locale.resolved() {
        Locale::ZhTw => format!("要強制停止 {name} 嗎？"),
        _ => format!("Force stop {name}?"),
    }
}

#[cfg(test)]
pub(crate) fn pseudo_long(value: &str) -> String {
    let expanded = value
        .chars()
        .flat_map(|character| [character, character])
        .collect::<String>();
    format!("⟦{expanded} · extended layout verification copy⟧")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_key_has_both_locales() {
        for key in CopyKey::ALL {
            assert!(!text(Locale::EnUs, key).trim().is_empty());
            assert!(!text(Locale::ZhTw, key).trim().is_empty());
        }
    }

    #[test]
    fn pseudo_long_copy_expands_every_catalog_value() {
        for key in CopyKey::ALL {
            let source = text(Locale::EnUs, key);
            let expanded = pseudo_long(source);
            assert!(expanded.chars().count() >= source.chars().count() * 2);
        }
    }

    #[test]
    fn pseudo_long_is_not_a_runtime_locale() {
        assert_eq!(Locale::parse("pseudo-long"), Locale::System);
    }
}
