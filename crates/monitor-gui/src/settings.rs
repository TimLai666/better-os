use std::{env, fs, path::PathBuf, time::Duration};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UnitBase {
    Decimal,
    #[default]
    Binary,
}

impl UnitBase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Decimal => "Decimal (kB, MB, GB)",
            Self::Binary => "Binary (KiB, MiB, GiB)",
        }
    }

    const fn config_value(self) -> &'static str {
        match self {
            Self::Decimal => "decimal",
            Self::Binary => "binary",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "decimal" => Self::Decimal,
            _ => Self::Binary,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TemperatureUnit {
    #[default]
    Celsius,
    Fahrenheit,
    Kelvin,
}

impl TemperatureUnit {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Celsius => "Celsius",
            Self::Fahrenheit => "Fahrenheit",
            Self::Kelvin => "Kelvin",
        }
    }

    const fn config_value(self) -> &'static str {
        match self {
            Self::Celsius => "celsius",
            Self::Fahrenheit => "fahrenheit",
            Self::Kelvin => "kelvin",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "fahrenheit" => Self::Fahrenheit,
            "kelvin" => Self::Kelvin,
            _ => Self::Celsius,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RefreshSpeed {
    VerySlow,
    Slow,
    #[default]
    Normal,
    Fast,
    VeryFast,
}

impl RefreshSpeed {
    pub const fn label(self) -> &'static str {
        match self {
            Self::VerySlow => "Very slow · 3 s",
            Self::Slow => "Slow · 2 s",
            Self::Normal => "Normal · 1 s",
            Self::Fast => "Fast · 500 ms",
            Self::VeryFast => "Very fast · 250 ms",
        }
    }

    pub const fn duration(self) -> Duration {
        match self {
            Self::VerySlow => Duration::from_secs(3),
            Self::Slow => Duration::from_secs(2),
            Self::Normal => Duration::from_secs(1),
            Self::Fast => Duration::from_millis(500),
            Self::VeryFast => Duration::from_millis(250),
        }
    }

    const fn config_value(self) -> &'static str {
        match self {
            Self::VerySlow => "very-slow",
            Self::Slow => "slow",
            Self::Normal => "normal",
            Self::Fast => "fast",
            Self::VeryFast => "very-fast",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "very-slow" => Self::VerySlow,
            "slow" => Self::Slow,
            "fast" => Self::Fast,
            "very-fast" => Self::VeryFast,
            _ => Self::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SidebarMeterType {
    #[default]
    ProgressBar,
    Graph,
}

impl SidebarMeterType {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ProgressBar => "Progress bars",
            Self::Graph => "Mini graphs",
        }
    }

    const fn config_value(self) -> &'static str {
        match self {
            Self::ProgressBar => "progress",
            Self::Graph => "graph",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "graph" => Self::Graph,
            _ => Self::ProgressBar,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProcessColumnSettings {
    pub pid: bool,
    pub user: bool,
    pub memory: bool,
    pub cpu: bool,
    pub read_speed: bool,
    pub read_total: bool,
    pub write_speed: bool,
    pub write_total: bool,
    pub gpu: bool,
    pub gpu_memory: bool,
    pub encoder: bool,
    pub decoder: bool,
    pub total_cpu_time: bool,
    pub user_cpu_time: bool,
    pub system_cpu_time: bool,
    pub priority: bool,
    pub swap: bool,
    pub combined_memory: bool,
    pub command_line: bool,
}

impl Default for ProcessColumnSettings {
    fn default() -> Self {
        Self {
            pid: true,
            user: true,
            memory: true,
            cpu: true,
            read_speed: true,
            read_total: false,
            write_speed: true,
            write_total: false,
            gpu: false,
            gpu_memory: false,
            encoder: false,
            decoder: false,
            total_cpu_time: false,
            user_cpu_time: false,
            system_cpu_time: false,
            priority: true,
            swap: false,
            combined_memory: false,
            command_line: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppColumnSettings {
    pub memory: bool,
    pub cpu: bool,
    pub read_speed: bool,
    pub read_total: bool,
    pub write_speed: bool,
    pub write_total: bool,
    pub gpu: bool,
    pub gpu_memory: bool,
    pub encoder: bool,
    pub decoder: bool,
    pub swap: bool,
    pub combined_memory: bool,
}

impl Default for AppColumnSettings {
    fn default() -> Self {
        Self {
            memory: true,
            cpu: true,
            read_speed: true,
            read_total: false,
            write_speed: true,
            write_total: false,
            gpu: false,
            gpu_memory: false,
            encoder: false,
            decoder: false,
            swap: false,
            combined_memory: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MonitorSettings {
    pub unit_base: UnitBase,
    pub temperature_unit: TemperatureUnit,
    pub refresh_speed: RefreshSpeed,
    pub sidebar_meter_type: SidebarMeterType,
    pub graph_data_points: usize,
    pub show_virtual_drives: bool,
    pub show_virtual_network_interfaces: bool,
    pub sidebar_details: bool,
    pub sidebar_description: bool,
    pub network_bits: bool,
    pub show_logical_cpus: bool,
    pub show_graph_grids: bool,
    pub normalize_cpu_usage: bool,
    pub detailed_priority: bool,
    pub app_columns: AppColumnSettings,
    pub process_columns: ProcessColumnSettings,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            unit_base: UnitBase::Binary,
            temperature_unit: TemperatureUnit::Celsius,
            refresh_speed: RefreshSpeed::Normal,
            sidebar_meter_type: SidebarMeterType::ProgressBar,
            graph_data_points: 120,
            show_virtual_drives: false,
            show_virtual_network_interfaces: false,
            sidebar_details: true,
            sidebar_description: true,
            network_bits: false,
            show_logical_cpus: false,
            show_graph_grids: true,
            normalize_cpu_usage: true,
            detailed_priority: false,
            app_columns: AppColumnSettings::default(),
            process_columns: ProcessColumnSettings::default(),
        }
    }
}

impl MonitorSettings {
    pub fn load() -> Self {
        let mut settings = Self::default();
        let Ok(content) = fs::read_to_string(Self::config_path()) else {
            return settings;
        };

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            settings.apply(key.trim(), value.trim());
        }
        settings
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_config())
    }

    pub fn refresh_interval(&self) -> Duration {
        self.refresh_speed.duration()
    }

    pub fn clamped_graph_points(&self) -> usize {
        self.graph_data_points.clamp(30, 600)
    }

    fn config_path() -> PathBuf {
        if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config_home)
                .join("better-os")
                .join("monitor.conf");
        }
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("better-os")
            .join("monitor.conf")
    }

    fn bool_value(value: &str) -> bool {
        matches!(value, "1" | "true" | "yes" | "on")
    }

    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "unit-base" => self.unit_base = UnitBase::parse(value),
            "temperature-unit" => self.temperature_unit = TemperatureUnit::parse(value),
            "refresh-speed" => self.refresh_speed = RefreshSpeed::parse(value),
            "sidebar-meter-type" => self.sidebar_meter_type = SidebarMeterType::parse(value),
            "graph-data-points" => {
                if let Ok(points) = value.parse::<usize>() {
                    self.graph_data_points = points.clamp(30, 600);
                }
            }
            "show-virtual-drives" => self.show_virtual_drives = Self::bool_value(value),
            "show-virtual-network-interfaces" => {
                self.show_virtual_network_interfaces = Self::bool_value(value)
            }
            "sidebar-details" => self.sidebar_details = Self::bool_value(value),
            "sidebar-description" => self.sidebar_description = Self::bool_value(value),
            "network-bits" => self.network_bits = Self::bool_value(value),
            "show-logical-cpus" => self.show_logical_cpus = Self::bool_value(value),
            "show-graph-grids" => self.show_graph_grids = Self::bool_value(value),
            "normalize-cpu-usage" => self.normalize_cpu_usage = Self::bool_value(value),
            "detailed-priority" => self.detailed_priority = Self::bool_value(value),
            "apps-show-memory" => self.app_columns.memory = Self::bool_value(value),
            "apps-show-cpu" => self.app_columns.cpu = Self::bool_value(value),
            "apps-show-drive-read-speed" => self.app_columns.read_speed = Self::bool_value(value),
            "apps-show-drive-read-total" => self.app_columns.read_total = Self::bool_value(value),
            "apps-show-drive-write-speed" => self.app_columns.write_speed = Self::bool_value(value),
            "apps-show-drive-write-total" => self.app_columns.write_total = Self::bool_value(value),
            "apps-show-gpu" => self.app_columns.gpu = Self::bool_value(value),
            "apps-show-gpu-memory" => self.app_columns.gpu_memory = Self::bool_value(value),
            "apps-show-encoder" => self.app_columns.encoder = Self::bool_value(value),
            "apps-show-decoder" => self.app_columns.decoder = Self::bool_value(value),
            "apps-show-swap" => self.app_columns.swap = Self::bool_value(value),
            "apps-show-combined-memory" => {
                self.app_columns.combined_memory = Self::bool_value(value)
            }
            "processes-show-id" => self.process_columns.pid = Self::bool_value(value),
            "processes-show-user" => self.process_columns.user = Self::bool_value(value),
            "processes-show-memory" => self.process_columns.memory = Self::bool_value(value),
            "processes-show-cpu" => self.process_columns.cpu = Self::bool_value(value),
            "processes-show-drive-read-speed" => {
                self.process_columns.read_speed = Self::bool_value(value)
            }
            "processes-show-drive-read-total" => {
                self.process_columns.read_total = Self::bool_value(value)
            }
            "processes-show-drive-write-speed" => {
                self.process_columns.write_speed = Self::bool_value(value)
            }
            "processes-show-drive-write-total" => {
                self.process_columns.write_total = Self::bool_value(value)
            }
            "processes-show-gpu" => self.process_columns.gpu = Self::bool_value(value),
            "processes-show-gpu-memory" => {
                self.process_columns.gpu_memory = Self::bool_value(value)
            }
            "processes-show-encoder" => self.process_columns.encoder = Self::bool_value(value),
            "processes-show-decoder" => self.process_columns.decoder = Self::bool_value(value),
            "processes-show-total-cpu-time" => {
                self.process_columns.total_cpu_time = Self::bool_value(value)
            }
            "processes-show-user-cpu-time" => {
                self.process_columns.user_cpu_time = Self::bool_value(value)
            }
            "processes-show-system-cpu-time" => {
                self.process_columns.system_cpu_time = Self::bool_value(value)
            }
            "processes-show-priority" => self.process_columns.priority = Self::bool_value(value),
            "processes-show-swap" => self.process_columns.swap = Self::bool_value(value),
            "processes-show-combined-memory" => {
                self.process_columns.combined_memory = Self::bool_value(value)
            }
            "processes-show-commandline" => {
                self.process_columns.command_line = Self::bool_value(value)
            }
            _ => {}
        }
    }

    fn to_config(&self) -> String {
        let mut lines = Vec::new();
        lines.push("# Better Monitor settings".to_string());
        lines.push(format!("unit-base={}", self.unit_base.config_value()));
        lines.push(format!(
            "temperature-unit={}",
            self.temperature_unit.config_value()
        ));
        lines.push(format!(
            "refresh-speed={}",
            self.refresh_speed.config_value()
        ));
        lines.push(format!(
            "sidebar-meter-type={}",
            self.sidebar_meter_type.config_value()
        ));
        lines.push(format!("graph-data-points={}", self.clamped_graph_points()));
        lines.push(format!("show-virtual-drives={}", self.show_virtual_drives));
        lines.push(format!(
            "show-virtual-network-interfaces={}",
            self.show_virtual_network_interfaces
        ));
        lines.push(format!("sidebar-details={}", self.sidebar_details));
        lines.push(format!("sidebar-description={}", self.sidebar_description));
        lines.push(format!("network-bits={}", self.network_bits));
        lines.push(format!("show-logical-cpus={}", self.show_logical_cpus));
        lines.push(format!("show-graph-grids={}", self.show_graph_grids));
        lines.push(format!("normalize-cpu-usage={}", self.normalize_cpu_usage));
        lines.push(format!("detailed-priority={}", self.detailed_priority));

        macro_rules! bool_line {
            ($name:literal, $value:expr) => {
                lines.push(format!("{}={}", $name, $value));
            };
        }

        bool_line!("apps-show-memory", self.app_columns.memory);
        bool_line!("apps-show-cpu", self.app_columns.cpu);
        bool_line!("apps-show-drive-read-speed", self.app_columns.read_speed);
        bool_line!("apps-show-drive-read-total", self.app_columns.read_total);
        bool_line!("apps-show-drive-write-speed", self.app_columns.write_speed);
        bool_line!("apps-show-drive-write-total", self.app_columns.write_total);
        bool_line!("apps-show-gpu", self.app_columns.gpu);
        bool_line!("apps-show-gpu-memory", self.app_columns.gpu_memory);
        bool_line!("apps-show-encoder", self.app_columns.encoder);
        bool_line!("apps-show-decoder", self.app_columns.decoder);
        bool_line!("apps-show-swap", self.app_columns.swap);
        bool_line!(
            "apps-show-combined-memory",
            self.app_columns.combined_memory
        );
        bool_line!("processes-show-id", self.process_columns.pid);
        bool_line!("processes-show-user", self.process_columns.user);
        bool_line!("processes-show-memory", self.process_columns.memory);
        bool_line!("processes-show-cpu", self.process_columns.cpu);
        bool_line!(
            "processes-show-drive-read-speed",
            self.process_columns.read_speed
        );
        bool_line!(
            "processes-show-drive-read-total",
            self.process_columns.read_total
        );
        bool_line!(
            "processes-show-drive-write-speed",
            self.process_columns.write_speed
        );
        bool_line!(
            "processes-show-drive-write-total",
            self.process_columns.write_total
        );
        bool_line!("processes-show-gpu", self.process_columns.gpu);
        bool_line!("processes-show-gpu-memory", self.process_columns.gpu_memory);
        bool_line!("processes-show-encoder", self.process_columns.encoder);
        bool_line!("processes-show-decoder", self.process_columns.decoder);
        bool_line!(
            "processes-show-total-cpu-time",
            self.process_columns.total_cpu_time
        );
        bool_line!(
            "processes-show-user-cpu-time",
            self.process_columns.user_cpu_time
        );
        bool_line!(
            "processes-show-system-cpu-time",
            self.process_columns.system_cpu_time
        );
        bool_line!("processes-show-priority", self.process_columns.priority);
        bool_line!("processes-show-swap", self.process_columns.swap);
        bool_line!(
            "processes-show-combined-memory",
            self.process_columns.combined_memory
        );
        bool_line!(
            "processes-show-commandline",
            self.process_columns.command_line
        );

        lines.push(String::new());
        lines.join("\n")
    }
}
