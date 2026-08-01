from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    source = file.read_text()
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected exactly one match, found {count}: {old[:80]!r}")
    file.write_text(source.replace(old, new, 1))


APP = "crates/monitor-gui/src/app.rs"
PARITY = "crates/monitor-gui/src/parity.rs"
DOC = "docs/better-monitor-resources-v1.10.2-parity.md"

replace_once(APP, "use better_ui::page_heading;", "use better_ui::{Locale, page_heading};")

replace_once(
    APP,
    '''    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Apps => "Apps",
            Self::Processes => "Processes",
            Self::Cpu => "Processor",
            Self::Memory => "Memory",
            Self::Gpu => "GPU",
            Self::Npu => "NPU",
            Self::Storage => "Drive",
            Self::Network => "Network Interface",
            Self::Battery => "Battery",
            Self::History => "History",
            Self::Incidents => "Incidents",
            Self::Diagnostics => "Diagnostics",
            Self::Settings => "Settings",
        }
    }
''',
    '''    fn label(self, locale: Locale) -> &'static str {
        match locale.resolved() {
            Locale::ZhTw => match self {
                Self::Overview => "總覽",
                Self::Apps => "應用程式",
                Self::Processes => "程序",
                Self::Cpu => "處理器",
                Self::Memory => "記憶體",
                Self::Gpu => "GPU",
                Self::Npu => "NPU",
                Self::Storage => "磁碟",
                Self::Network => "網路介面",
                Self::Battery => "電池",
                Self::History => "歷史記錄",
                Self::Incidents => "事件標記",
                Self::Diagnostics => "診斷",
                Self::Settings => "設定",
            },
            _ => match self {
                Self::Overview => "Overview",
                Self::Apps => "Apps",
                Self::Processes => "Processes",
                Self::Cpu => "Processor",
                Self::Memory => "Memory",
                Self::Gpu => "GPU",
                Self::Npu => "NPU",
                Self::Storage => "Drive",
                Self::Network => "Network Interface",
                Self::Battery => "Battery",
                Self::History => "History",
                Self::Incidents => "Incidents",
                Self::Diagnostics => "Diagnostics",
                Self::Settings => "Settings",
            },
        }
    }
''',
)

replace_once(
    APP,
    '''    const fn subtitle(self) -> &'static str {
        match self {
            Self::Overview => "Current resource activity and observation coverage",
            Self::Apps => "Application groups, resource columns, search, details, and controls",
            Self::Processes => "Sortable process metrics, search, details, and controls",
            Self::Cpu => "Total or logical CPU usage, clocks, temperature, topology, and uptime",
            Self::Memory => "Memory, swap, availability, and hardware-property coverage",
            Self::Gpu => "Usage, media engines, memory, thermals, power, clocks, and driver",
            Self::Npu => "Usage, memory, thermals, power, clocks, and driver",
            Self::Storage => "Per-drive activity, throughput, totals, capacity, and properties",
            Self::Network => "Per-interface traffic, totals, link, driver, and identity",
            Self::Battery => "Charge, power, health, capacity, cycles, and identity",
            Self::History => "Recent bounded samples and Better Monitor incident markers",
            Self::Incidents => "User-marked slowdown moments and evidence capture boundaries",
            Self::Diagnostics => "Collector health, support states, and observation blind spots",
            Self::Settings => "Refresh, units, sidebar, graphs, devices, and table columns",
        }
    }
''',
    '''    fn subtitle(self, locale: Locale) -> &'static str {
        match locale.resolved() {
            Locale::ZhTw => match self {
                Self::Overview => "目前的資源活動與資料收集涵蓋範圍",
                Self::Apps => "應用程式群組、資源欄位、搜尋、資訊與控制",
                Self::Processes => "可排序的程序指標、搜尋、資訊與控制",
                Self::Cpu => "整體或邏輯 CPU 使用率、時脈、溫度、拓撲與運作時間",
                Self::Memory => "記憶體、交換空間、可用量與硬體資訊涵蓋範圍",
                Self::Gpu => "使用率、媒體引擎、記憶體、溫度、功耗、時脈與驅動程式",
                Self::Npu => "使用率、記憶體、溫度、功耗、時脈與驅動程式",
                Self::Storage => "各磁碟活動、吞吐量、累計量、容量與屬性",
                Self::Network => "各介面流量、累計量、連線、驅動程式與識別資訊",
                Self::Battery => "電量、功耗、健康度、容量、循環次數與識別資訊",
                Self::History => "近期有限長度的樣本與 Better Monitor 事件標記",
                Self::Incidents => "使用者標記的變慢時刻與證據擷取邊界",
                Self::Diagnostics => "資料收集器健康狀態、支援狀態與觀測盲點",
                Self::Settings => "更新頻率、單位、側邊欄、圖表、裝置與表格欄位",
            },
            _ => match self {
                Self::Overview => "Current resource activity and observation coverage",
                Self::Apps => "Application groups, resource columns, search, details, and controls",
                Self::Processes => "Sortable process metrics, search, details, and controls",
                Self::Cpu => "Total or logical CPU usage, clocks, temperature, topology, and uptime",
                Self::Memory => "Memory, swap, availability, and hardware-property coverage",
                Self::Gpu => "Usage, media engines, memory, thermals, power, clocks, and driver",
                Self::Npu => "Usage, memory, thermals, power, clocks, and driver",
                Self::Storage => "Per-drive activity, throughput, totals, capacity, and properties",
                Self::Network => "Per-interface traffic, totals, link, driver, and identity",
                Self::Battery => "Charge, power, health, capacity, cycles, and identity",
                Self::History => "Recent bounded samples and Better Monitor incident markers",
                Self::Incidents => "User-marked slowdown moments and evidence capture boundaries",
                Self::Diagnostics => "Collector health, support states, and observation blind spots",
                Self::Settings => "Refresh, units, sidebar, graphs, devices, and table columns",
            },
        }
    }
''',
)

replace_once(
    APP,
    '.label(format!("{}   {}", page.marker(), page.label()))',
    '.label(format!("{}   {}", page.marker(), page.label(self.settings.locale)))',
)
replace_once(
    APP,
    '.label(format!("{}  {}", page.marker(), page.label()))',
    '.label(format!("{}  {}", page.marker(), page.label(self.settings.locale)))',
)
replace_once(
    APP,
    '.child(page_heading(self.active_page.label()))',
    '.child(page_heading(self.active_page.label(self.settings.locale)))',
)
replace_once(
    APP,
    '.child(self.active_page.subtitle()),',
    '.child(self.active_page.subtitle(self.settings.locale)),',
)
replace_once(
    APP,
    '''Button::new("overview-page")
                            .ghost()
                            .small()
                            .label("Overview")''',
    '''Button::new("overview-page")
                            .ghost()
                            .small()
                            .label(match self.settings.locale.resolved() {
                                Locale::ZhTw => "總覽",
                                _ => "Overview",
                            })''',
)
replace_once(
    APP,
    '''.label(if self.charts_paused {
                                "Resume graphs"
                            } else {
                                "Pause graphs"
                            })''',
    '''.label(match (self.settings.locale.resolved(), self.charts_paused) {
                                (Locale::ZhTw, true) => "繼續更新圖表",
                                (Locale::ZhTw, false) => "暫停更新圖表",
                                (_, true) => "Resume graphs",
                                (_, false) => "Pause graphs",
                            })''',
)

replace_once(
    PARITY,
    '''        let point = self.current_point();
        let memory_detail =''',
    '''        let point = self.current_point();
        let locale = self.settings.locale.resolved();
        let memory_detail =''',
)
replace_once(
    PARITY,
    '''Button::new("sidebar-settings")
                            .ghost()
                            .small()
                            .label("Settings")''',
    '''Button::new("sidebar-settings")
                            .ghost()
                            .small()
                            .label(match locale {
                                Locale::ZhTw => "設定",
                                _ => "Settings",
                            })''',
)
replace_once(
    PARITY,
    '''.child(self.sidebar_group_label("Applications", cx))
                        .child(self.sidebar_resource_row(
                            "sidebar-apps",
                            "Apps".to_string(),''',
    '''.child(self.sidebar_group_label(
                            match locale {
                                Locale::ZhTw => "應用程式",
                                _ => "Applications",
                            },
                            cx,
                        ))
                        .child(self.sidebar_resource_row(
                            "sidebar-apps",
                            MonitorPage::Apps.label(locale).to_string(),''',
)
replace_once(PARITY, '"Processes".to_string(),', 'MonitorPage::Processes.label(locale).to_string(),')
replace_once(
    PARITY,
    '.child(self.sidebar_group_label("System", cx))',
    '''.child(self.sidebar_group_label(
                            match locale {
                                Locale::ZhTw => "系統",
                                _ => "System",
                            },
                            cx,
                        ))''',
)
replace_once(PARITY, '"Processor".to_string(),', 'MonitorPage::Cpu.label(locale).to_string(),')
replace_once(PARITY, '"Memory".to_string(),', 'MonitorPage::Memory.label(locale).to_string(),')
replace_once(PARITY, '"Drive".to_string(),', 'MonitorPage::Storage.label(locale).to_string(),')
replace_once(PARITY, '"Battery".to_string(),', 'MonitorPage::Battery.label(locale).to_string(),')
replace_once(
    PARITY,
    '.child(div().text_sm().font_bold().child("Recording")),',
    '''.child(div().text_sm().font_bold().child(match locale {
                                Locale::ZhTw => "正在記錄",
                                _ => "Recording",
                            })),''',
)
replace_once(
    PARITY,
    '''"{} samples · {}",
                                self.store.samples().len(),
                                self.settings.refresh_speed.label()''',
    '''"{} {} · {}",
                                self.store.samples().len(),
                                match locale {
                                    Locale::ZhTw => "筆樣本",
                                    _ => "samples",
                                },
                                self.settings.refresh_speed.label()''',
)
replace_once(
    PARITY,
    '''                    v_flex()
                        .gap_2()
                        .child(
                            self.setting_row(
                                "Refresh speed",''',
    '''                    v_flex()
                        .gap_2()
                        .child(
                            self.setting_row(
                                match self.settings.locale.resolved() {
                                    Locale::ZhTw => "語言",
                                    _ => "Language",
                                },
                                match self.settings.locale.resolved() {
                                    Locale::ZhTw => "切換後立即套用，不需要重新啟動",
                                    _ => "Changes apply immediately without restarting",
                                },
                                Button::new("setting-locale")
                                    .outline()
                                    .small()
                                    .label(self.settings.locale.label())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.locale = this.settings.locale.next();
                                        this.persist_settings();
                                        cx.notify();
                                    })),
                                cx,
                            ),
                        )
                        .child(
                            self.setting_row(
                                "Refresh speed",''',
)

replace_once(
    DOC,
    '| Runtime language selector | ⬜ | `en-US`, `zh-TW`, system language, and pseudo-long test mode are missing. |',
    '| Runtime language selector | 🟨 | Shared `system`, `en-US`, and `zh-TW` locale state is persisted and switches shell/navigation copy immediately. Full page catalogs and pseudo-long mode remain incomplete. |',
)
replace_once(
    DOC,
    '''| `en-US` | 🟨 | English strings exist but are not in a locale catalog. |
| `zh-TW` | ⬜ | Missing. |
| System language | ⬜ | Missing. |
| Runtime switching | ⬜ | Missing. |''',
    '''| `en-US` | 🟨 | Shell, navigation, selected-page headings, and language settings are locale-driven; remaining page copy is still being cataloged. |
| `zh-TW` | 🟨 | Shell, navigation, selected-page headings, and language settings have Traditional Chinese copy; remaining page copy is incomplete. |
| System language | ✅ | Shared Better OS locale resolves `LANG` to `zh-TW` or `en-US`. |
| Runtime switching | 🟨 | Monitor switches and persists locale without restart; search placeholders, table headers, dialogs, and full page content still need catalog coverage. |''',
)
