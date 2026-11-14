//! Every word the window can show, in both shipped languages.
//!
//! The pattern matches Better Manager: one `Copy` struct, one constant per
//! locale, and a compile error the moment a string is added to one language
//! and forgotten in the other. Sentences that carry a value (a name, a count,
//! a time) are functions below the two constants, with both languages side
//! by side.
//!
//! Nothing here shows an internal name. A metric, an inventory key, a
//! collector, or a process state the window has no words for is left out,
//! never printed as its key.

use better_ui::fluent::list::CellTone;
use better_ui::fluent::words;
use monitor_store::{
    Detection, Inventory, InventoryDiff, Sensitivity, SlowdownCause, SlowdownKind,
    SlowdownResource, SlowdownRules,
};
use monitor_views::ProcessFacts;
use monitor_views::grouping::EvidenceKind;
use monitor_views::overview::{OverviewModel, ResourceVerdict};

use crate::app::EndTarget;
use crate::charts::{AdapterKind, Readings};
use crate::startup::{Activity, Scope, ServiceLock, ServiceUnit, StartupState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Locale {
    System,
    EnUs,
    ZhTw,
}

impl Locale {
    /// The order the language setting lists them in.
    pub(crate) const ALL: [Locale; 3] = [Locale::System, Locale::EnUs, Locale::ZhTw];

    pub(crate) fn resolved(self) -> Self {
        match self {
            Self::System => match words::Locale::from_environment() {
                words::Locale::TraditionalChinese => Self::ZhTw,
                words::Locale::English => Self::EnUs,
            },
            locale => locale,
        }
    }

    pub(crate) fn is_chinese(self) -> bool {
        self.resolved() == Self::ZhTw
    }

    /// The kit's locale, for the words its controls draw themselves.
    pub(crate) fn words(self) -> words::Locale {
        if self.is_chinese() {
            words::Locale::TraditionalChinese
        } else {
            words::Locale::English
        }
    }

    /// The stored word.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::EnUs => "en",
            Self::ZhTw => "zh-TW",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "system" => Some(Self::System),
            "en" => Some(Self::EnUs),
            "zh-TW" => Some(Self::ZhTw),
            _ => None,
        }
    }
}

pub(crate) struct Copy {
    pub(crate) app_name: &'static str,
    pub(crate) overview: &'static str,
    pub(crate) processes: &'static str,
    pub(crate) details: &'static str,
    pub(crate) performance: &'static str,
    pub(crate) startup_apps: &'static str,
    pub(crate) services: &'static str,
    pub(crate) history: &'static str,
    pub(crate) events: &'static str,
    pub(crate) system_info: &'static str,
    pub(crate) settings: &'static str,
    pub(crate) search_placeholder: &'static str,
    pub(crate) pause_updates: &'static str,
    pub(crate) resume_updates: &'static str,
    pub(crate) paused_title: &'static str,
    pub(crate) paused_message: &'static str,
    pub(crate) end_task: &'static str,
    pub(crate) end_process_tree: &'static str,
    pub(crate) properties: &'static str,
    pub(crate) tree_view: &'static str,
    pub(crate) enable: &'static str,
    pub(crate) disable: &'static str,
    pub(crate) refresh: &'static str,
    pub(crate) open_file_location: &'static str,
    pub(crate) go_to_details: &'static str,
    pub(crate) pause_process: &'static str,
    pub(crate) resume_process: &'static str,
    pub(crate) lower_priority: &'static str,

    // Resources.
    pub(crate) cpu: &'static str,
    pub(crate) memory: &'static str,
    pub(crate) disk: &'static str,
    pub(crate) network: &'static str,
    pub(crate) gpu: &'static str,
    pub(crate) battery: &'static str,
    pub(crate) temperatures: &'static str,

    // Overview.
    pub(crate) using_the_most: &'static str,
    pub(crate) waiting_for_first_reading: &'static str,
    pub(crate) no_chart: &'static str,
    pub(crate) not_available: &'static str,
    pub(crate) not_read: &'static str,
    pub(crate) gpu_not_found: &'static str,

    // Tables.
    pub(crate) apps_section: &'static str,
    pub(crate) background_processes: &'static str,
    pub(crate) nothing_matches: &'static str,
    pub(crate) column_name: &'static str,
    pub(crate) column_pid: &'static str,
    pub(crate) column_parent_pid: &'static str,
    pub(crate) column_user: &'static str,
    pub(crate) column_state: &'static str,
    pub(crate) column_cpu: &'static str,
    pub(crate) column_cpu_time: &'static str,
    pub(crate) column_memory: &'static str,
    pub(crate) column_swap: &'static str,
    pub(crate) column_disk: &'static str,
    pub(crate) column_read: &'static str,
    pub(crate) column_write: &'static str,
    pub(crate) column_threads: &'static str,
    pub(crate) column_descriptors: &'static str,
    pub(crate) column_running_for: &'static str,
    pub(crate) column_nice: &'static str,
    pub(crate) column_cgroup: &'static str,
    pub(crate) column_command_line: &'static str,
    pub(crate) column_status: &'static str,
    pub(crate) column_starts_for: &'static str,
    pub(crate) column_command: &'static str,
    pub(crate) column_service_name: &'static str,
    pub(crate) column_at_login: &'static str,
    pub(crate) state_column_tooltip: &'static str,
    pub(crate) disk_column_tooltip: &'static str,
    pub(crate) partial_total: &'static str,
    pub(crate) seconds_old: &'static str,

    // Why a reading has no value.
    pub(crate) not_yet_sampled: &'static str,
    pub(crate) interval_too_short: &'static str,
    pub(crate) read_failed: &'static str,
    pub(crate) malformed: &'static str,
    pub(crate) entity_disappeared: &'static str,
    pub(crate) interface_missing: &'static str,
    pub(crate) not_reported: &'static str,
    pub(crate) policy_withheld: &'static str,
    pub(crate) permission_denied: &'static str,
    pub(crate) not_collected: &'static str,

    // Grouping.
    pub(crate) evidence_systemd_unit: &'static str,
    pub(crate) evidence_flatpak: &'static str,
    pub(crate) evidence_snap: &'static str,
    pub(crate) evidence_desktop: &'static str,
    pub(crate) evidence_ancestry: &'static str,
    pub(crate) evidence_executable: &'static str,
    pub(crate) evidence_unattributed: &'static str,
    pub(crate) confidence_high: &'static str,
    pub(crate) confidence_medium: &'static str,
    pub(crate) confidence_low: &'static str,

    // Actions.
    pub(crate) outcome_signal_sent: &'static str,
    pub(crate) outcome_priority_changed: &'static str,
    pub(crate) failure_disappeared: &'static str,
    pub(crate) failure_denied: &'static str,
    pub(crate) failure_invalid: &'static str,
    pub(crate) failure_unsupported: &'static str,
    pub(crate) failure_other: &'static str,
    pub(crate) refusal_other_user: &'static str,
    pub(crate) refusal_ownership_unknown: &'static str,
    pub(crate) refusal_protected: &'static str,
    pub(crate) refusal_raise_priority: &'static str,
    pub(crate) refusal_nice_range: &'static str,
    pub(crate) refusal_priority_unknown: &'static str,
    pub(crate) refusal_protected_tree: &'static str,
    pub(crate) list_paused: &'static str,
    pub(crate) no_location: &'static str,
    pub(crate) switched_on: &'static str,
    pub(crate) switched_off: &'static str,
    pub(crate) switch_failed: &'static str,

    // Performance.
    pub(crate) not_reported_yet: &'static str,
    pub(crate) sixty_seconds: &'static str,
    pub(crate) overall_utilization: &'static str,
    pub(crate) logical_processors: &'static str,
    pub(crate) utilization: &'static str,
    pub(crate) utilization_percent: &'static str,
    pub(crate) utilization_per_processor: &'static str,
    pub(crate) speed: &'static str,
    pub(crate) maximum_speed: &'static str,
    pub(crate) processes_count: &'static str,
    pub(crate) threads: &'static str,
    pub(crate) up_time: &'static str,
    pub(crate) waiting_to_run: &'static str,
    pub(crate) waiting_to_run_hint: &'static str,
    pub(crate) temperature: &'static str,
    pub(crate) memory_usage: &'static str,
    pub(crate) memory_composition: &'static str,
    pub(crate) memory_hint: &'static str,
    pub(crate) in_use: &'static str,
    pub(crate) modified: &'static str,
    pub(crate) cached: &'static str,
    pub(crate) free: &'static str,
    pub(crate) available: &'static str,
    pub(crate) committed: &'static str,
    pub(crate) swap: &'static str,
    pub(crate) swap_off: &'static str,
    pub(crate) active_time: &'static str,
    pub(crate) active_time_short: &'static str,
    pub(crate) transfer_rate: &'static str,
    pub(crate) read_solid_write_dashed: &'static str,
    pub(crate) response_time: &'static str,
    pub(crate) read_speed: &'static str,
    pub(crate) write_speed: &'static str,
    pub(crate) capacity: &'static str,
    pub(crate) kind: &'static str,
    pub(crate) ssd: &'static str,
    pub(crate) hdd: &'static str,
    pub(crate) throughput: &'static str,
    pub(crate) receive_solid_send_dashed: &'static str,
    pub(crate) send: &'static str,
    pub(crate) receive: &'static str,
    pub(crate) connection: &'static str,
    pub(crate) link_speed: &'static str,
    pub(crate) wireless: &'static str,
    pub(crate) wired: &'static str,
    pub(crate) other_connection: &'static str,
    pub(crate) processor_temperature: &'static str,
    pub(crate) column_network: &'static str,
    pub(crate) column_gpu: &'static str,
    pub(crate) column_gpu_memory: &'static str,
    pub(crate) column_power: &'static str,
    pub(crate) network_column_tooltip: &'static str,
    pub(crate) npu: &'static str,
    pub(crate) sensors: &'static str,
    pub(crate) dedicated_memory: &'static str,
    pub(crate) shared_memory: &'static str,
    pub(crate) gpu_clock: &'static str,
    pub(crate) memory_clock: &'static str,
    pub(crate) power: &'static str,
    pub(crate) driver: &'static str,
    pub(crate) location: &'static str,
    pub(crate) programs_using: &'static str,
    pub(crate) busy_from_clients: &'static str,
    pub(crate) gpu_apps: &'static str,
    pub(crate) gpu_busy_unreported: &'static str,
    pub(crate) firmware: &'static str,
    pub(crate) power_state: &'static str,
    pub(crate) npu_asleep: &'static str,
    pub(crate) npu_running: &'static str,
    pub(crate) awake_time: &'static str,
    pub(crate) npu_no_busy: &'static str,
    pub(crate) npu_idle: &'static str,
    pub(crate) charge: &'static str,
    pub(crate) battery_charging: &'static str,
    pub(crate) battery_discharging: &'static str,
    pub(crate) battery_full: &'static str,
    pub(crate) battery_not_charging: &'static str,
    pub(crate) battery_unknown: &'static str,
    pub(crate) time_left: &'static str,
    pub(crate) time_to_full: &'static str,
    pub(crate) battery_health: &'static str,
    pub(crate) full_capacity: &'static str,
    pub(crate) design_capacity: &'static str,
    pub(crate) charge_cycles: &'static str,
    pub(crate) not_reported_by_firmware: &'static str,
    pub(crate) voltage: &'static str,
    pub(crate) charge_limit: &'static str,
    pub(crate) charging_at: &'static str,
    pub(crate) drawing: &'static str,
    pub(crate) battery_model: &'static str,
    pub(crate) chargers: &'static str,
    pub(crate) connected: &'static str,
    pub(crate) not_connected: &'static str,
    pub(crate) power_adapter: &'static str,
    pub(crate) usb_c_port: &'static str,
    pub(crate) battery_charge_chart: &'static str,
    pub(crate) battery_use_title: &'static str,
    pub(crate) battery_use_now: &'static str,
    pub(crate) battery_use_since: &'static str,
    pub(crate) battery_use_note: &'static str,
    pub(crate) battery_use_plugged: &'static str,
    pub(crate) battery_use_never: &'static str,
    pub(crate) battery_use_nothing: &'static str,
    pub(crate) column_energy: &'static str,
    pub(crate) no_battery: &'static str,
    pub(crate) temperatures_heading: &'static str,
    pub(crate) fans: &'static str,
    pub(crate) voltages: &'static str,
    pub(crate) currents: &'static str,
    pub(crate) power_readings: &'static str,
    pub(crate) clocks: &'static str,
    pub(crate) no_fans: &'static str,
    pub(crate) no_sensors: &'static str,
    pub(crate) critical_at: &'static str,
    pub(crate) hottest: &'static str,
    pub(crate) fan_speed: &'static str,
    pub(crate) open_full_window: &'static str,
    pub(crate) collection_version_mismatch: &'static str,
    pub(crate) collection_component_off: &'static str,
    pub(crate) popup_last_minutes: &'static str,

    // Startup apps and services.
    pub(crate) startup_hint: &'static str,
    pub(crate) enabled: &'static str,
    pub(crate) disabled: &'static str,
    pub(crate) not_on_this_desktop: &'static str,
    pub(crate) not_installed: &'static str,
    pub(crate) not_installed_hint: &'static str,
    pub(crate) no_startup_apps: &'static str,
    pub(crate) loading: &'static str,
    pub(crate) services_hint: &'static str,
    pub(crate) no_services: &'static str,
    pub(crate) no_service_manager: &'static str,
    pub(crate) no_service_manager_advice: &'static str,

    // Settings.
    pub(crate) appearance_and_language: &'static str,
    pub(crate) appearance: &'static str,
    pub(crate) appearance_description: &'static str,
    pub(crate) dark_theme: &'static str,
    pub(crate) light_theme: &'static str,
    pub(crate) system_theme: &'static str,
    pub(crate) language: &'static str,
    pub(crate) language_description: &'static str,
    pub(crate) system_default: &'static str,
    pub(crate) privacy: &'static str,
    pub(crate) show_command_lines: &'static str,
    pub(crate) privacy_description: &'static str,
    pub(crate) how_it_works: &'static str,
    pub(crate) grouping_title: &'static str,
    pub(crate) grouping_description: &'static str,
    pub(crate) data_sources: &'static str,
    pub(crate) readings_this_second: &'static str,
    pub(crate) sampling: &'static str,
    pub(crate) sampling_description: &'static str,
    pub(crate) health_healthy: &'static str,
    pub(crate) health_degraded: &'static str,
    pub(crate) health_failed: &'static str,
    pub(crate) health_unsupported: &'static str,

    // Where the numbers come from.
    pub(crate) collection_connecting: &'static str,
    pub(crate) collection_service: &'static str,
    pub(crate) collection_in_process_title: &'static str,
    pub(crate) collection_in_process: &'static str,
    pub(crate) collection_unavailable_title: &'static str,
    pub(crate) collection_unavailable: &'static str,
    pub(crate) request_failed: &'static str,

    // History.
    pub(crate) history_subtitle: &'static str,
    pub(crate) span_15_minutes: &'static str,
    pub(crate) span_1_hour: &'static str,
    pub(crate) span_6_hours: &'static str,
    pub(crate) no_history_yet: &'static str,
    pub(crate) no_history_explained: &'static str,
    pub(crate) history_truncated: &'static str,
    pub(crate) history_processor: &'static str,
    pub(crate) history_memory: &'static str,
    pub(crate) history_disk: &'static str,
    pub(crate) history_network: &'static str,
    pub(crate) history_waiting: &'static str,
    pub(crate) stored_samples: &'static str,
    pub(crate) one_sample_every: &'static str,
    pub(crate) kept_for: &'static str,
    pub(crate) disk_budget: &'static str,
    pub(crate) gaps_title: &'static str,
    pub(crate) no_gaps: &'static str,
    pub(crate) set_aside_title: &'static str,
    pub(crate) gap_service_stopped: &'static str,
    pub(crate) gap_missed_cadence: &'static str,
    pub(crate) gap_interrupted_write: &'static str,
    pub(crate) gap_retention: &'static str,

    // Events.
    pub(crate) events_intro_title: &'static str,
    pub(crate) events_rules_title: &'static str,
    pub(crate) events_not_recording: &'static str,
    pub(crate) no_events_yet: &'static str,
    pub(crate) no_events_explained: &'static str,
    pub(crate) recorded_slowdowns: &'static str,
    pub(crate) what_changed: &'static str,
    pub(crate) baseline_none: &'static str,
    pub(crate) busiest_then: &'static str,

    // System information.
    pub(crate) inventory_never_captured: &'static str,
    pub(crate) inventory_first_capture: &'static str,
    pub(crate) inventory_no_changes: &'static str,
    pub(crate) changes_title: &'static str,
    pub(crate) changes_since_last: &'static str,
    pub(crate) stays_on_this_computer: &'static str,
}

pub(crate) const EN: Copy = Copy {
    app_name: "Better Monitor",
    overview: "Overview",
    processes: "Processes",
    details: "Details",
    performance: "Performance",
    startup_apps: "Startup apps",
    services: "Services",
    history: "History",
    events: "Slow moments",
    system_info: "System information",
    settings: "Settings",
    search_placeholder: "Search by name, PID, or user",
    pause_updates: "Pause",
    resume_updates: "Resume",
    paused_title: "Updates are paused",
    paused_message: "The numbers on screen are frozen. Better Monitor keeps recording in the background.",
    end_task: "End task",
    end_process_tree: "End process tree",
    properties: "Properties",
    tree_view: "Tree view",
    enable: "Enable",
    disable: "Disable",
    refresh: "Refresh",
    open_file_location: "Open file location",
    go_to_details: "Go to details",
    pause_process: "Suspend",
    resume_process: "Resume",
    lower_priority: "Lower priority",

    cpu: "Processor",
    memory: "Memory",
    disk: "Disk",
    network: "Network",
    gpu: "Graphics card",
    battery: "Battery",
    temperatures: "Temperatures",

    using_the_most: "Using the most right now",
    waiting_for_first_reading: "Waiting for the first reading…",
    no_chart: "No chart: this isn't measured yet",
    not_available: "Not available",
    not_read: "Not measured",
    gpu_not_found: "No graphics card found",

    apps_section: "Apps",
    background_processes: "Background processes",
    nothing_matches: "Nothing matches your search.",
    column_name: "Name",
    column_pid: "PID",
    column_parent_pid: "Parent PID",
    column_user: "User",
    column_state: "Status",
    column_cpu: "CPU",
    column_cpu_time: "CPU time",
    column_memory: "Memory",
    column_swap: "Swap",
    column_disk: "Disk",
    column_read: "Read",
    column_write: "Write",
    column_threads: "Threads",
    column_descriptors: "Open files",
    column_running_for: "Running for",
    column_nice: "Priority",
    column_cgroup: "System group",
    column_command_line: "Command line",
    column_status: "Status",
    column_starts_for: "Starts for",
    column_command: "Command",
    column_service_name: "Service name",
    column_at_login: "At login",
    state_column_tooltip: "What the process is doing. Point at a status to see what it means.",
    disk_column_tooltip: "How fast the process reads from or writes to disk, per second.",
    partial_total: "At least this much. Some processes didn't report:",
    seconds_old: "s old",

    not_yet_sampled: "Measuring…",
    interval_too_short: "Measuring…",
    read_failed: "Couldn't read",
    malformed: "Couldn't read",
    entity_disappeared: "Gone",
    interface_missing: "Not on this system",
    not_reported: "Not reported",
    policy_withheld: "Hidden for privacy",
    permission_denied: "Needs permission",
    not_collected: "Not measured",

    evidence_systemd_unit: "Grouped because they run in the same system service.",
    evidence_flatpak: "Grouped because they belong to the same Flatpak app.",
    evidence_snap: "Grouped because they belong to the same Snap.",
    evidence_desktop: "Grouped because your desktop started them as one app.",
    evidence_ancestry: "Grouped because one started the others.",
    evidence_executable: "Shown on its own: nothing ties it to another process.",
    evidence_unattributed: "Shown on its own: there was nothing to group it by.",
    confidence_high: "This is certain.",
    confidence_medium: "This is likely.",
    confidence_low: "This is a best guess.",

    outcome_signal_sent: "Asked it to close.",
    outcome_priority_changed: "Priority lowered",
    failure_disappeared: "It had already closed.",
    failure_denied: "You don't have permission to do that.",
    failure_invalid: "That can't be done to this process.",
    failure_unsupported: "That isn't supported here.",
    failure_other: "That didn't work. Try again.",
    refusal_other_user: "It belongs to another user, so you can't change it here.",
    refusal_ownership_unknown: "Its owner couldn't be read, so it isn't safe to change.",
    refusal_protected: "The system needs it, so it can't be ended here.",
    refusal_raise_priority: "Raising priority needs administrator permission.",
    refusal_nice_range: "Its priority is already as low as it goes.",
    refusal_priority_unknown: "Its current priority couldn't be read.",
    refusal_protected_tree: "Its tree holds part of your session, such as the desktop, a login shell, or Better Monitor itself, so the whole tree can't be ended here.",
    list_paused: "Updates are paused, so the list may show a process that has already closed. Resume updates, then end it.",
    no_location: "Its program's folder couldn't be opened. It may belong to another user or to the system.",
    switched_on: "Turned on. It starts the next time you sign in.",
    switched_off: "Turned off. It won't start the next time you sign in.",
    switch_failed: "That couldn't be changed:",

    not_reported_yet: "Nothing has been measured yet.",
    sixty_seconds: "60 seconds",
    overall_utilization: "Overall",
    logical_processors: "Logical processors",
    utilization: "Utilization",
    utilization_percent: "% Utilization",
    utilization_per_processor: "% Utilization of each logical processor",
    speed: "Speed",
    maximum_speed: "Maximum speed",
    processes_count: "Processes",
    threads: "Threads",
    up_time: "Up time",
    waiting_to_run: "Waiting to run (1 minute average)",
    waiting_to_run_hint: "How many tasks were running or waiting for a processor on average. Below the number of logical processors, nothing waited long.",
    temperature: "Temperature",
    memory_usage: "Memory in use",
    memory_composition: "Memory composition",
    memory_hint: "Cached memory holds recently used files. Programs get it back as soon as they need it, so it counts as available.",
    in_use: "In use",
    modified: "Waiting to be saved",
    cached: "Cached",
    free: "Free",
    available: "Available",
    committed: "Committed",
    swap: "Swap",
    swap_off: "Off",
    active_time: "Active time",
    active_time_short: "Active time",
    transfer_rate: "Transfer rate",
    read_solid_write_dashed: "Read (solid) · Write (dashed)",
    response_time: "Response time",
    read_speed: "Read speed",
    write_speed: "Write speed",
    capacity: "Capacity",
    kind: "Type",
    ssd: "SSD",
    hdd: "Hard disk",
    throughput: "Throughput",
    receive_solid_send_dashed: "Receive (solid) · Send (dashed)",
    send: "Send",
    receive: "Receive",
    connection: "Connection",
    link_speed: "Link speed",
    wireless: "Wi‑Fi",
    wired: "Ethernet",
    other_connection: "Network",
    processor_temperature: "Processor temperature",
    column_network: "Network",
    column_gpu: "GPU",
    column_gpu_memory: "GPU memory",
    column_power: "Power use",
    network_column_tooltip: "TCP traffic per second. UDP traffic, such as a browser's QUIC connections, has no per-program counter and isn't counted.",
    npu: "NPU",
    sensors: "Temperatures and fans",
    dedicated_memory: "Dedicated GPU memory",
    shared_memory: "Shared GPU memory",
    gpu_clock: "GPU clock",
    memory_clock: "Memory clock",
    power: "Power",
    driver: "Driver",
    location: "Location",
    programs_using: "Programs using it",
    busy_from_clients: "How busy it is comes from your own programs' use; other users' programs aren't counted.",
    gpu_apps: "Apps using the graphics card",
    gpu_busy_unreported: "The driver doesn't report how busy this card is.",
    firmware: "Firmware",
    power_state: "Power state",
    npu_asleep: "Asleep",
    npu_running: "Running",
    awake_time: "Awake time",
    npu_no_busy: "The driver doesn't report how busy the NPU is while programs use it.",
    npu_idle: "No program has the NPU open, so it isn't doing any work.",
    charge: "Charge",
    battery_charging: "Charging",
    battery_discharging: "On battery",
    battery_full: "Fully charged",
    battery_not_charging: "Plugged in, not charging",
    battery_unknown: "State not reported",
    time_left: "Time left",
    time_to_full: "Until full",
    battery_health: "Battery health",
    full_capacity: "Full charge capacity",
    design_capacity: "Design capacity",
    charge_cycles: "Charge cycles",
    not_reported_by_firmware: "Not reported by the firmware",
    voltage: "Voltage",
    charge_limit: "Charge limit",
    charging_at: "Charging power",
    drawing: "Power draw",
    battery_model: "Model",
    chargers: "Power adapters",
    connected: "Connected",
    not_connected: "Not connected",
    power_adapter: "Power adapter",
    usb_c_port: "USB-C port",
    battery_charge_chart: "Charge (%)",
    battery_use_title: "What is using the battery",
    battery_use_now: "Right now",
    battery_use_since: "Since unplugged",
    battery_use_note: "An estimate: the battery's draw is shared out by each app's processor and graphics time. The screen and the radios aren't charged to any app.",
    battery_use_plugged: "Plugged in, so nothing is using the battery now.",
    battery_use_never: "Better Monitor hasn't seen this computer run on battery since it started.",
    battery_use_nothing: "No app has used a measurable share yet.",
    column_energy: "Energy",
    no_battery: "This computer has no battery.",
    temperatures_heading: "Temperatures",
    fans: "Fans",
    voltages: "Voltages",
    currents: "Currents",
    power_readings: "Power",
    clocks: "Clocks",
    no_fans: "This computer reports no fan.",
    no_sensors: "This computer reports no sensors.",
    critical_at: "Critical at",
    hottest: "Hottest",
    fan_speed: "Fan speed",
    open_full_window: "Open Better Monitor",
    collection_component_off: "Better Monitor is turned off in Better Manager, so its background service isn't running and isn't started from here. This window collects for itself while it is open.",
    collection_version_mismatch: "Better Monitor was updated, but its background service is still the older version, so this window is collecting for itself. The service restarts with the new version at your next sign-in.",
    popup_last_minutes: "Last 5 minutes",

    startup_hint: "Apps that start when you sign in. Turning one off writes your own copy of its setting; files installed for everyone are never changed.",
    enabled: "Enabled",
    disabled: "Disabled",
    not_on_this_desktop: "Not for this desktop",
    not_installed: "Not installed",
    not_installed_hint: "The program this entry starts is no longer installed, so it won't start whatever its switch says.",
    no_startup_apps: "No apps start when you sign in.",
    loading: "Loading…",
    services_hint: "Background services your session runs. Turning one on or off needs no administrator permission. Services that belong to Better OS components are managed by Better Manager.",
    no_services: "Your session runs no background services.",
    no_service_manager: "Your session's service manager couldn't be reached.",
    no_service_manager_advice: "Sign out and back in. If it keeps happening, the session may have started without a user service manager.",

    appearance_and_language: "Appearance and language",
    appearance: "Appearance",
    appearance_description: "Follows your desktop's light or dark mode unless you pick one",
    dark_theme: "Dark",
    light_theme: "Light",
    system_theme: "Use system setting",
    language: "Language",
    language_description: "The language Better Monitor uses",
    system_default: "Use system setting",
    privacy: "Privacy",
    show_command_lines: "Show command lines",
    privacy_description: "Command lines can contain passwords and private paths. They are only read while this is on.",
    how_it_works: "How it works",
    grouping_title: "How processes are grouped into apps",
    grouping_description: "Evidence is used in this order. Processes are never merged just because their names match.",
    data_sources: "Where the numbers come from",
    readings_this_second: "Readings this second",
    sampling: "Measured every second",
    sampling_description: "Measuring happens outside the window. Pausing the screen doesn't stop it.",
    health_healthy: "Working",
    health_degraded: "Partly working",
    health_failed: "Not working",
    health_unsupported: "Not available here",

    collection_connecting: "Connecting to the Better Monitor service…",
    collection_service: "The Better Monitor service is recording. It keeps recording after you close this window.",
    collection_in_process_title: "Recording only while this window is open",
    collection_in_process: "The Better Monitor service isn't running, so this window is measuring for itself. History stops when you close it.",
    collection_unavailable_title: "Nothing can be measured",
    collection_unavailable: "Neither the Better Monitor service nor this window could start measuring.",
    request_failed: "That request didn't work",

    history_subtitle: "How the computer has been doing, from what Better Monitor recorded.",
    span_15_minutes: "Last 15 minutes",
    span_1_hour: "Last hour",
    span_6_hours: "Last 6 hours",
    no_history_yet: "Nothing recorded for this period yet",
    no_history_explained: "History builds up while the Better Monitor service or this window is running. Times when neither ran are gaps, not quiet times.",
    history_truncated: "Only the newest part of this period is shown.",
    history_processor: "Processor utilization",
    history_memory: "Memory in use",
    history_disk: "Disk read and write",
    history_network: "Network send and receive",
    history_waiting: "Time programs waited for a resource",
    stored_samples: "Recorded moments",
    one_sample_every: "One moment every",
    kept_for: "Kept for",
    disk_budget: "Disk space allowed",
    gaps_title: "Times nothing was recorded",
    no_gaps: "Nothing is missing in this period.",
    set_aside_title: "Some saved readings could not be read",
    gap_service_stopped: "Better Monitor wasn't running",
    gap_missed_cadence: "Measuring fell behind",
    gap_interrupted_write: "Recording was interrupted",
    gap_retention: "Older than what is kept",

    events_intro_title: "Better Monitor writes these down by itself as they happen. There is nothing to press.",
    events_rules_title: "What counts as a slowdown",
    events_not_recording: "Nothing is recording right now, so a slowdown can't be noticed. Start the Better Monitor service or keep this window open.",
    no_events_yet: "No slowdowns recorded yet",
    no_events_explained: "When one happens, it appears here: what programs were waiting for, for how long, and which programs were using the most.",
    recorded_slowdowns: "Recorded slowdowns",
    what_changed: "What changed compared with just before",
    baseline_none: "Nothing measured changed noticeably.",
    busiest_then: "Busiest programs at that moment",

    inventory_never_captured: "Better Monitor hasn't looked at this computer's hardware and software yet.",
    inventory_first_capture: "This is the first look, so there is nothing to compare yet",
    inventory_no_changes: "Nothing changed since the last look",
    changes_title: "Changes",
    changes_since_last: "Changes since the last look",
    stays_on_this_computer: "stays on this computer",
};

pub(crate) const ZH: Copy = Copy {
    app_name: "系統監控",
    overview: "總覽",
    processes: "處理程序",
    details: "詳細資料",
    performance: "效能",
    startup_apps: "啟動應用程式",
    services: "服務",
    history: "歷程記錄",
    events: "變慢記錄",
    system_info: "系統資訊",
    settings: "設定",
    search_placeholder: "依名稱、PID 或使用者搜尋",
    pause_updates: "暫停",
    resume_updates: "繼續",
    paused_title: "畫面更新已暫停",
    paused_message: "畫面上的數字停在暫停那一刻，系統監控仍在背景繼續記錄。",
    end_task: "結束工作",
    end_process_tree: "結束處理程序樹狀結構",
    properties: "內容",
    tree_view: "樹狀檢視",
    enable: "啟用",
    disable: "停用",
    refresh: "重新整理",
    open_file_location: "開啟檔案位置",
    go_to_details: "移至詳細資料",
    pause_process: "暫停",
    resume_process: "繼續",
    lower_priority: "降低優先順序",

    cpu: "處理器",
    memory: "記憶體",
    disk: "磁碟",
    network: "網路",
    gpu: "顯示卡",
    battery: "電池",
    temperatures: "溫度",

    using_the_most: "目前用最多的程式",
    waiting_for_first_reading: "正在等第一筆數據…",
    no_chart: "沒有圖表：目前還沒測量這項",
    not_available: "無法取得",
    not_read: "未測量",
    gpu_not_found: "找不到顯示卡",

    apps_section: "應用程式",
    background_processes: "背景處理程序",
    nothing_matches: "沒有符合搜尋的項目。",
    column_name: "名稱",
    column_pid: "PID",
    column_parent_pid: "父 PID",
    column_user: "使用者",
    column_state: "狀態",
    column_cpu: "CPU",
    column_cpu_time: "CPU 時間",
    column_memory: "記憶體",
    column_swap: "置換空間",
    column_disk: "磁碟",
    column_read: "讀取",
    column_write: "寫入",
    column_threads: "執行緒",
    column_descriptors: "開啟的檔案",
    column_running_for: "已執行",
    column_nice: "優先順序",
    column_cgroup: "系統群組",
    column_command_line: "命令列",
    column_status: "狀態",
    column_starts_for: "適用對象",
    column_command: "命令",
    column_service_name: "服務名稱",
    column_at_login: "登入時",
    state_column_tooltip: "處理程序正在做什麼。指標停在狀態上可以看說明。",
    disk_column_tooltip: "處理程序每秒從磁碟讀取或寫入多少資料。",
    partial_total: "至少這麼多，有些處理程序沒有回報：",
    seconds_old: "秒前",

    not_yet_sampled: "測量中…",
    interval_too_short: "測量中…",
    read_failed: "無法讀取",
    malformed: "無法讀取",
    entity_disappeared: "已消失",
    interface_missing: "這台電腦沒有",
    not_reported: "未回報",
    policy_withheld: "為保護隱私而隱藏",
    permission_denied: "需要權限",
    not_collected: "未測量",

    evidence_systemd_unit: "歸在一起，因為它們在同一個系統服務裡執行。",
    evidence_flatpak: "歸在一起，因為它們屬於同一個 Flatpak 應用程式。",
    evidence_snap: "歸在一起，因為它們屬於同一個 Snap。",
    evidence_desktop: "歸在一起，因為桌面把它們當成同一個應用程式啟動。",
    evidence_ancestry: "歸在一起，因為其他處理程序都是由它啟動的。",
    evidence_executable: "單獨列出：沒有任何線索把它和其他處理程序連在一起。",
    evidence_unattributed: "單獨列出：沒有可以用來歸類的資料。",
    confidence_high: "這點可以確定。",
    confidence_medium: "這點大致可信。",
    confidence_low: "這只是最合理的推測。",

    outcome_signal_sent: "已要求它關閉。",
    outcome_priority_changed: "已降低優先順序",
    failure_disappeared: "它已經關閉了。",
    failure_denied: "你沒有權限這麼做。",
    failure_invalid: "這個處理程序不能這樣操作。",
    failure_unsupported: "這裡不支援這個操作。",
    failure_other: "沒有成功，請再試一次。",
    refusal_other_user: "它屬於其他使用者，這裡不能變更。",
    refusal_ownership_unknown: "讀不到它的擁有者，變更它並不安全。",
    refusal_protected: "系統需要它，這裡不能結束它。",
    refusal_raise_priority: "提高優先順序需要系統管理員權限。",
    refusal_nice_range: "它的優先順序已經是最低了。",
    refusal_priority_unknown: "讀不到它目前的優先順序。",
    refusal_protected_tree: "它的樹狀結構裡有你這次登入的一部分，例如桌面、登入的 shell 或系統監控本身，所以不能在這裡整個結束。",
    list_paused: "畫面更新已暫停，清單可能還列著已經關閉的處理程序。請先繼續更新，再結束它。",
    no_location: "無法開啟它的程式所在的資料夾，它可能屬於其他使用者或系統。",
    switched_on: "已開啟，下次登入時會啟動。",
    switched_off: "已關閉，下次登入時不會啟動。",
    switch_failed: "無法變更：",

    not_reported_yet: "目前還沒有測量到任何數據。",
    sixty_seconds: "60 秒",
    overall_utilization: "整體",
    logical_processors: "邏輯處理器",
    utilization: "使用率",
    utilization_percent: "使用率 %",
    utilization_per_processor: "每個邏輯處理器的使用率 %",
    speed: "速度",
    maximum_speed: "最高速度",
    processes_count: "處理程序",
    threads: "執行緒",
    up_time: "開機時間",
    waiting_to_run: "等待執行（1 分鐘平均）",
    waiting_to_run_hint: "平均有多少工作正在執行或等著用處理器。只要低於邏輯處理器的數量，就沒有工作等很久。",
    temperature: "溫度",
    memory_usage: "使用中的記憶體",
    memory_composition: "記憶體組成",
    memory_hint: "快取的記憶體存放最近用過的檔案，程式一需要就會拿回來用，所以也算在可用記憶體裡。",
    in_use: "使用中",
    modified: "等待寫入",
    cached: "快取",
    free: "閒置",
    available: "可用",
    committed: "已認可",
    swap: "置換空間",
    swap_off: "未開啟",
    active_time: "使用時間",
    active_time_short: "使用時間",
    transfer_rate: "傳輸速度",
    read_solid_write_dashed: "讀取（實線）・寫入（虛線）",
    response_time: "回應時間",
    read_speed: "讀取速度",
    write_speed: "寫入速度",
    capacity: "容量",
    kind: "類型",
    ssd: "固態硬碟",
    hdd: "傳統硬碟",
    throughput: "流量",
    receive_solid_send_dashed: "接收（實線）・傳送（虛線）",
    send: "傳送",
    receive: "接收",
    connection: "連線",
    link_speed: "連線速度",
    wireless: "Wi‑Fi",
    wired: "乙太網路",
    other_connection: "網路",
    processor_temperature: "處理器溫度",
    column_network: "網路",
    column_gpu: "GPU",
    column_gpu_memory: "GPU 記憶體",
    column_power: "耗電",
    network_column_tooltip: "每秒的 TCP 流量。UDP 流量（例如瀏覽器的 QUIC 連線）沒有各程式的計數，所以不算在內。",
    npu: "NPU",
    sensors: "溫度與風扇",
    dedicated_memory: "專用 GPU 記憶體",
    shared_memory: "共用 GPU 記憶體",
    gpu_clock: "GPU 時脈",
    memory_clock: "記憶體時脈",
    power: "功率",
    driver: "驅動程式",
    location: "位置",
    programs_using: "使用中的程式",
    busy_from_clients: "忙碌程度是加總你自己程式的用量，其他使用者的程式不算在內。",
    gpu_apps: "使用顯示卡的應用程式",
    gpu_busy_unreported: "驅動程式沒有回報這張顯示卡有多忙。",
    firmware: "韌體",
    power_state: "電源狀態",
    npu_asleep: "休眠中",
    npu_running: "運作中",
    awake_time: "喚醒時間",
    npu_no_busy: "驅動程式沒有回報程式使用 NPU 時它有多忙。",
    npu_idle: "目前沒有程式開啟 NPU，所以它沒有在工作。",
    charge: "電量",
    battery_charging: "充電中",
    battery_discharging: "使用電池中",
    battery_full: "已充飽",
    battery_not_charging: "已接上電源，未充電",
    battery_unknown: "狀態未回報",
    time_left: "剩餘時間",
    time_to_full: "充滿還需",
    battery_health: "電池健康度",
    full_capacity: "完全充電容量",
    design_capacity: "設計容量",
    charge_cycles: "充電循環次數",
    not_reported_by_firmware: "韌體沒有提供",
    voltage: "電壓",
    charge_limit: "充電上限",
    charging_at: "充電功率",
    drawing: "耗電功率",
    battery_model: "型號",
    chargers: "電源轉接器",
    connected: "已連接",
    not_connected: "未連接",
    power_adapter: "電源轉接器",
    usb_c_port: "USB-C 連接埠",
    battery_charge_chart: "電量（%）",
    battery_use_title: "哪些程式在耗電",
    battery_use_now: "現在",
    battery_use_since: "拔掉電源之後",
    battery_use_note: "這是估算：依各應用程式使用處理器和顯示卡的時間，分攤電池的耗電。螢幕和無線網路的耗電不算在任何程式頭上。",
    battery_use_plugged: "目前接著電源，現在沒有程式在耗用電池。",
    battery_use_never: "系統監控啟動之後，還沒看到這台電腦使用電池。",
    battery_use_nothing: "還沒有應用程式用到可量測的電量。",
    column_energy: "耗電量",
    no_battery: "這台電腦沒有電池。",
    temperatures_heading: "溫度",
    fans: "風扇",
    voltages: "電壓",
    currents: "電流",
    power_readings: "功率",
    clocks: "時脈",
    no_fans: "這台電腦沒有回報風扇。",
    no_sensors: "這台電腦沒有回報任何感測器。",
    critical_at: "臨界溫度",
    hottest: "最高溫",
    fan_speed: "風扇轉速",
    open_full_window: "開啟系統監控",
    collection_component_off: "系統監控在 Better Manager 裡是關閉的，所以背景服務沒有執行，這裡也不會啟動它。這個視窗開著的時候會自己收集資料。",
    collection_version_mismatch: "系統監控已更新，但背景服務還是舊版本，所以這個視窗先自己收集資料。下次登入時服務會以新版本重新啟動。",
    popup_last_minutes: "最近 5 分鐘",

    startup_hint: "登入時會自動啟動的應用程式。關閉時只會寫入你自己的設定副本，不會改動為所有人安裝的檔案。",
    enabled: "已啟用",
    disabled: "已停用",
    not_on_this_desktop: "不適用這個桌面",
    not_installed: "程式未安裝",
    not_installed_hint: "這個項目要啟動的程式已經不在了，所以不管開關怎麼設都不會啟動。",
    no_startup_apps: "登入時沒有會自動啟動的應用程式。",
    loading: "正在載入…",
    services_hint: "你這次登入執行的背景服務。開啟或關閉不需要系統管理員權限。屬於 Better OS 元件的服務由元件管理器管理。",
    no_services: "你這次登入沒有執行任何背景服務。",
    no_service_manager: "連不到你這次登入的服務管理員。",
    no_service_manager_advice: "請登出再重新登入。如果一直發生，可能是這次登入沒有啟動使用者服務管理員。",

    appearance_and_language: "外觀與語言",
    appearance: "外觀",
    appearance_description: "預設跟著桌面切換淺色或深色，也可以固定用其中一種",
    dark_theme: "深色",
    light_theme: "淺色",
    system_theme: "跟隨系統",
    language: "語言",
    language_description: "系統監控使用的語言",
    system_default: "跟隨系統設定",
    privacy: "隱私",
    show_command_lines: "顯示命令列",
    privacy_description: "命令列可能含有密碼和私人路徑，只有開啟這項時才會讀取。",
    how_it_works: "運作方式",
    grouping_title: "處理程序如何歸類成應用程式",
    grouping_description: "依照這個順序採用線索。不會只因為名稱相同就把處理程序合併。",
    data_sources: "數據從哪裡來",
    readings_this_second: "這一秒的數據",
    sampling: "每秒測量一次",
    sampling_description: "測量在視窗之外進行，暫停畫面不會停止測量。",
    health_healthy: "正常",
    health_degraded: "部分正常",
    health_failed: "無法運作",
    health_unsupported: "這台電腦無法使用",

    collection_connecting: "正在連線到系統監控服務…",
    collection_service: "系統監控服務正在記錄，關閉這個視窗後仍會繼續記錄。",
    collection_in_process_title: "只在這個視窗開著時記錄",
    collection_in_process: "系統監控服務沒有執行，所以由這個視窗自己測量。關閉視窗後就不會再記錄歷程。",
    collection_unavailable_title: "無法測量任何數據",
    collection_unavailable: "系統監控服務和這個視窗都無法開始測量。",
    request_failed: "這個要求沒有成功",

    history_subtitle: "根據系統監控記錄下來的數據，看看電腦這段時間的狀況。",
    span_15_minutes: "最近 15 分鐘",
    span_1_hour: "最近 1 小時",
    span_6_hours: "最近 6 小時",
    no_history_yet: "這段期間還沒有記錄",
    no_history_explained: "系統監控服務或這個視窗執行時才會累積歷程。兩者都沒執行的時段是空白，不代表電腦很閒。",
    history_truncated: "這段期間只顯示最新的部分。",
    history_processor: "處理器使用率",
    history_memory: "使用中的記憶體",
    history_disk: "磁碟讀取與寫入",
    history_network: "網路傳送與接收",
    history_waiting: "程式等待資源的時間",
    stored_samples: "記錄的時間點",
    one_sample_every: "記錄間隔",
    kept_for: "保留時間",
    disk_budget: "可用的磁碟空間",
    gaps_title: "沒有記錄的時段",
    no_gaps: "這段期間沒有缺漏。",
    set_aside_title: "有些已存的紀錄讀不出來",
    gap_service_stopped: "系統監控沒有執行",
    gap_missed_cadence: "測量跟不上",
    gap_interrupted_write: "記錄中斷",
    gap_retention: "超過保留時間",

    events_intro_title: "系統監控會在電腦變慢的當下自動記下來，不需要按任何東西。",
    events_rules_title: "怎樣算是變慢",
    events_not_recording: "目前沒有東西在記錄，所以發現不了變慢。請啟動系統監控服務，或讓這個視窗保持開啟。",
    no_events_yet: "還沒有記錄到變慢",
    no_events_explained: "發生的時候會列在這裡：程式在等什麼、等了多久，以及哪些程式用得最多。",
    recorded_slowdowns: "記錄到的變慢",
    what_changed: "和前一段時間相比的變化",
    baseline_none: "測量到的數據沒有明顯變化。",
    busiest_then: "當時最忙的程式",

    inventory_never_captured: "系統監控還沒查看這台電腦的硬體和軟體。",
    inventory_first_capture: "這是第一次查看，還沒有可以比較的資料",
    inventory_no_changes: "和上次查看相比沒有變化",
    changes_title: "變化",
    changes_since_last: "和上次查看相比的變化",
    stays_on_this_computer: "只留在這台電腦",
};

pub(crate) fn copy(locale: Locale) -> &'static Copy {
    match locale.resolved() {
        Locale::ZhTw => &ZH,
        _ => &EN,
    }
}

fn zh(locale: Locale) -> bool {
    locale.is_chinese()
}

/// How the machine is doing, as a color beside the sentence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tone {
    Good,
    Busy,
    Strained,
    Unknown,
}

fn strained(verdict: &ResourceVerdict) -> bool {
    matches!(
        verdict,
        ResourceVerdict::UnderPressure { .. } | ResourceVerdict::Saturated { .. }
    )
}

fn join(parts: &[&str], locale: Locale) -> String {
    if zh(locale) {
        return parts.join("、");
    }
    match parts {
        [] => String::new(),
        [one] => one.to_string(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

/// The one plain sentence at the top of the Overview, and a second line
/// naming the program behind it when there is one.
///
/// "Busy" and "waiting" are kept apart because they look the same in a
/// utilization number and mean opposite things: a busy machine is getting
/// work done, a machine where programs wait is the one that feels slow.
pub(crate) fn health_sentence(
    overview: &OverviewModel,
    readings: &Readings,
    busiest: Option<&str>,
    locale: Locale,
) -> (Tone, String, Option<String>) {
    let z = zh(locale);
    if !readings.has_cpu && !readings.has_memory {
        return (
            Tone::Unknown,
            if z {
                "正在了解電腦的狀況…"
            } else {
                "Checking how your computer is doing…"
            }
            .to_string(),
            None,
        );
    }
    let detail = busiest.map(|name| {
        if z {
            format!("目前用最多處理器時間的是「{name}」。")
        } else {
            format!("{name} is using the most processor time right now.")
        }
    });
    let mut waiting: Vec<&str> = Vec::new();
    if strained(&overview.cpu.verdict) {
        waiting.push(if z { "處理器" } else { "the processor" });
    }
    if strained(&overview.memory.resource.verdict) {
        waiting.push(if z { "記憶體" } else { "memory" });
    }
    if strained(&overview.io.verdict) {
        waiting.push(if z { "磁碟" } else { "the disk" });
    }
    if !waiting.is_empty() {
        let list = join(&waiting, locale);
        return (
            Tone::Strained,
            if z {
                format!("電腦變慢了：有程式在等{list}。")
            } else {
                format!("Your computer is slowing down: programs are waiting for {list}.")
            },
            detail,
        );
    }
    let memory_tight = readings
        .memory
        .in_use_fraction()
        .is_some_and(|fraction| fraction >= 0.9);
    if memory_tight {
        return (
            Tone::Busy,
            if z {
                "記憶體快用完了，不過還沒有程式需要等待。"
            } else {
                "Memory is almost full, but nothing is waiting yet."
            }
            .to_string(),
            detail,
        );
    }
    let busy = matches!(overview.cpu.verdict, ResourceVerdict::BusyWithoutContention)
        || matches!(overview.io.verdict, ResourceVerdict::BusyWithoutContention);
    if busy {
        return (
            Tone::Busy,
            if z {
                "電腦正在忙，但沒有程式需要等待。"
            } else {
                "Your computer is busy, but nothing is waiting."
            }
            .to_string(),
            detail,
        );
    }
    (
        Tone::Good,
        if z {
            "電腦運作順暢。"
        } else {
            "Your computer is running smoothly."
        }
        .to_string(),
        detail,
    )
}

/// A resource's state in a word or two.
pub(crate) fn verdict_word(verdict: &ResourceVerdict, locale: Locale) -> &'static str {
    let z = zh(locale);
    match verdict {
        ResourceVerdict::Nominal => {
            if z {
                "正常"
            } else {
                "Normal"
            }
        }
        ResourceVerdict::BusyWithoutContention => {
            if z {
                "忙碌"
            } else {
                "Busy"
            }
        }
        ResourceVerdict::UnderPressure { .. } => {
            if z {
                "有程式在等"
            } else {
                "Programs waiting"
            }
        }
        ResourceVerdict::Saturated { .. } => {
            if z {
                "超載"
            } else {
                "Overloaded"
            }
        }
        ResourceVerdict::CollectorFailed { .. } => {
            if z {
                "無法讀取"
            } else {
                "Can't read"
            }
        }
        ResourceVerdict::Unsupported { .. } | ResourceVerdict::Unobserved { .. } => {
            if z {
                "未測量"
            } else {
                "Not measured"
            }
        }
    }
}

/// A process's kernel state, in words a person knows, with what it means.
pub(crate) fn process_state(raw: &str, locale: Locale) -> (&'static str, &'static str) {
    let z = zh(locale);
    let pick = |en: (&'static str, &'static str), zh: (&'static str, &'static str)| {
        if z { zh } else { en }
    };
    match raw {
        "running" => pick(
            ("Running", "Using a processor right now."),
            ("執行中", "正在使用處理器。"),
        ),
        "sleeping" => pick(
            (
                "Waiting",
                "Waiting for input, a timer, or data. This is normal.",
            ),
            ("等待中", "正在等輸入、計時或資料，這是正常狀態。"),
        ),
        "uninterruptible" => pick(
            (
                "Waiting for disk",
                "Waiting for a disk or another device, and can't be interrupted until it answers.",
            ),
            ("等候磁碟", "正在等磁碟或其他裝置回應，回應前無法中斷。"),
        ),
        "idle" => pick(
            ("Idle", "A system task with nothing to do."),
            ("閒置", "沒有工作的系統工作。"),
        ),
        "zombie" => pick(
            (
                "Ended",
                "It has ended, and the program that started it hasn't collected its result yet.",
            ),
            ("已結束", "已經結束，但啟動它的程式還沒收回結果。"),
        ),
        "stopped" => pick(
            ("Suspended", "Paused. It does nothing until it is resumed."),
            ("已暫停", "已暫停，繼續之前不會做任何事。"),
        ),
        "tracing-stop" => pick(
            ("Paused by debugger", "Stopped by a debugger."),
            ("偵錯暫停", "被偵錯工具暫停。"),
        ),
        "dead" => pick(
            ("Ending", "It is being removed."),
            ("正在結束", "正在移除中。"),
        ),
        "paging" | "wakekill" | "parked" => pick(
            ("Waiting", "Waiting inside the system."),
            ("等待中", "正在系統內部等待。"),
        ),
        _ => pick(
            (
                "Unknown",
                "The system reported a state Better Monitor doesn't know.",
            ),
            ("未知", "系統回報了系統監控不認得的狀態。"),
        ),
    }
}

pub(crate) fn logical_processors(count: u64, locale: Locale) -> String {
    if zh(locale) {
        format!("{count} 個邏輯處理器")
    } else {
        format!("{count} logical processors")
    }
}

pub(crate) fn of_total(used: &str, total: &str, locale: Locale) -> String {
    if zh(locale) {
        format!("已使用 {used}，共 {total}")
    } else {
        format!("{used} of {total} in use")
    }
}

pub(crate) fn active_share(share: &str, locale: Locale) -> String {
    if zh(locale) {
        format!("忙碌 {share}")
    } else {
        format!("{share} busy")
    }
}

fn or_missing(value: Option<String>, locale: Locale) -> String {
    value.unwrap_or_else(|| copy(locale).not_available.to_string())
}

pub(crate) fn read_write(read: Option<String>, write: Option<String>, locale: Locale) -> String {
    let (read, write) = (or_missing(read, locale), or_missing(write, locale));
    if zh(locale) {
        format!("讀取 {read} · 寫入 {write}")
    } else {
        format!("Read {read} · Write {write}")
    }
}

pub(crate) fn receive_send(
    receive: Option<String>,
    send: Option<String>,
    locale: Locale,
) -> String {
    let (receive, send) = (or_missing(receive, locale), or_missing(send, locale));
    if zh(locale) {
        format!("接收 {receive} · 傳送 {send}")
    } else {
        format!("Receive {receive} · Send {send}")
    }
}

/// Receive and send in a list row, where there is room for little else.
pub(crate) fn receive_send_short(
    receive: Option<String>,
    send: Option<String>,
    locale: Locale,
) -> String {
    format!(
        "↓ {}  ↑ {}",
        or_missing(receive, locale),
        or_missing(send, locale)
    )
}

pub(crate) fn adapter_status(connected: Option<bool>, locale: Locale) -> &'static str {
    let z = zh(locale);
    match connected {
        Some(true) => {
            if z {
                "已連線"
            } else {
                "Connected"
            }
        }
        Some(false) => {
            if z {
                "未連線"
            } else {
                "Not connected"
            }
        }
        None => {
            if z {
                "狀態未知"
            } else {
                "Status unknown"
            }
        }
    }
}

pub(crate) fn adapter_title(kind: AdapterKind, name: &str, locale: Locale) -> String {
    let c = copy(locale);
    match kind {
        AdapterKind::Wireless => format!("{} ({name})", c.wireless),
        AdapterKind::Wired => format!("{} ({name})", c.wired),
        AdapterKind::Other => format!("{} ({name})", c.other_connection),
    }
}

pub(crate) fn disk_title(name: &str, _rotational: Option<bool>, locale: Locale) -> String {
    format!("{} ({name})", copy(locale).disk)
}

/// A link speed in bits per second, as network settings write it.
pub(crate) fn bits_per_second(value: f64) -> String {
    if value >= 1e9 {
        format!("{:.1} Gbps", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.0} Mbps", value / 1e6)
    } else {
        format!("{:.0} Kbps", value / 1e3)
    }
}

/// How long the machine has been on.
pub(crate) fn uptime(seconds: u64, locale: Locale) -> String {
    let days = seconds / 86_400;
    if days == 0 {
        return words::duration(locale.words(), seconds);
    }
    let hours = (seconds % 86_400) / 3_600;
    if zh(locale) {
        format!("{days} 天 {hours} 小時")
    } else if days == 1 {
        format!("1 day {hours} h")
    } else {
        format!("{days} days {hours} h")
    }
}

pub(crate) fn seconds(value: u64, locale: Locale) -> String {
    words::duration(locale.words(), value)
}

pub(crate) fn version_line(version: &str, locale: Locale) -> String {
    if zh(locale) {
        format!("版本 {version}")
    } else {
        format!("Version {version}")
    }
}

/// "End …?" and what it will do.
pub(crate) fn end_question(target: &EndTarget, count: usize, locale: Locale) -> (String, String) {
    let z = zh(locale);
    match target {
        EndTarget::Process { name, .. } => (
            if z {
                format!("要結束「{name}」嗎？")
            } else {
                format!("End {name}?")
            },
            if z {
                "會要求這個處理程序關閉，還沒儲存的內容可能會遺失。".to_string()
            } else {
                "The process is asked to close. Anything it hasn't saved may be lost.".to_string()
            },
        ),
        EndTarget::Tree { name, .. } => (
            if z {
                format!("要結束「{name}」和它啟動的所有處理程序嗎？")
            } else {
                format!("End {name} and everything it started?")
            },
            if z && count <= 1 {
                "它沒有啟動其他處理程序，只會要求它關閉。還沒儲存的內容可能會遺失。".to_string()
            } else if z {
                format!("會要求 {count} 個處理程序關閉，從它啟動的開始。還沒儲存的內容可能會遺失。")
            } else if count <= 1 {
                "It started nothing else, so only it is asked to close. Anything it hasn't saved may be lost.".to_string()
            } else {
                format!(
                    "{count} processes are asked to close, starting with the ones it started. Anything they haven't saved may be lost."
                )
            },
        ),
        EndTarget::App { name, .. } => (
            if z {
                format!("要結束「{name}」嗎？")
            } else {
                format!("End {name}?")
            },
            if z {
                format!("會要求它的 {count} 個處理程序全部關閉，還沒儲存的內容可能會遺失。")
            } else if count == 1 {
                "Its process is asked to close. Anything it hasn't saved may be lost.".to_string()
            } else {
                format!(
                    "All {count} of its processes are asked to close. Anything they haven't saved may be lost."
                )
            },
        ),
    }
}

/// The details under a failed service switch: what systemctl said, or why
/// nothing was asked of it. Never the main message.
pub(crate) fn service_error_detail(error: &crate::startup::ServiceError, locale: Locale) -> String {
    use crate::startup::ServiceError;
    match error {
        ServiceError::NoManager(detail) | ServiceError::Refused(detail) => {
            detail.trim().to_string()
        }
        ServiceError::Locked(lock) => lock_sentence(*lock, locale).to_string(),
        ServiceError::InvalidName(name) => {
            if zh(locale) {
                format!("「{name}」不是一般的服務名稱，所以沒有交給 systemctl。")
            } else {
                format!("\"{name}\" isn't a plain service name, so it was not passed to systemctl.")
            }
        }
    }
}

/// The one sentence a failed switch says.
pub(crate) fn switch_failed_title(name: &str, locale: Locale) -> String {
    if zh(locale) {
        format!("無法變更「{name}」。")
    } else {
        format!("{name} couldn't be changed.")
    }
}

/// Why a switch did not take, with what the system said when it said
/// anything.
pub(crate) fn switch_failed_sentence(detail: &str, locale: Locale) -> String {
    let lead = copy(locale).switch_failed.trim_end_matches([':', '：']);
    let detail = detail.trim();
    if detail.is_empty() {
        if zh(locale) {
            format!("{lead}。")
        } else {
            format!("{lead}.")
        }
    } else if zh(locale) {
        format!("{lead}：{detail}")
    } else {
        format!("{lead}: {detail}")
    }
}

pub(crate) fn asked_to_close_many(count: usize, locale: Locale) -> String {
    if zh(locale) {
        format!("已要求 {count} 個處理程序關閉。")
    } else {
        format!("Asked {count} processes to close.")
    }
}

/// Everything known about a process, as label, value, and whether the value
/// is a reason rather than a reading.
pub(crate) fn properties(
    process: &ProcessFacts,
    locale: Locale,
    processors: Option<f64>,
) -> Vec<(String, String, bool)> {
    use monitor_views::ProcessColumn as Col;
    let c = copy(locale);
    let mut rows = Vec::new();
    for column in [
        Col::Pid,
        Col::ParentPid,
        Col::State,
        Col::User,
        Col::CpuUtilization,
        Col::CpuTime,
        Col::Memory,
        Col::Swap,
        Col::ReadRate,
        Col::WriteRate,
        Col::Threads,
        Col::FileDescriptors,
        Col::StartTime,
        Col::Nice,
        Col::Cgroup,
        Col::CommandLine,
    ] {
        let cell = crate::tables::process_cell(process, column, locale, processors);
        let mut value = match (&cell.tone, &cell.tooltip) {
            (CellTone::Secondary, Some(reason)) => reason.to_string(),
            _ => cell.text.to_string(),
        };
        if column == Col::State
            && let Some(explanation) = &cell.tooltip
        {
            value = format!("{value} — {explanation}");
        }
        rows.push((
            crate::tables::column_title(column, c).to_string(),
            value,
            cell.tone == CellTone::Secondary,
        ));
    }
    rows
}

/// A link failure, said plainly. The key behind it is never shown.
pub(crate) fn failure_sentence(detail: &str, locale: Locale) -> String {
    let z = zh(locale);
    if detail.contains("privacy_owned_by_service") {
        return if z {
            "系統監控服務自己決定要收集哪些資料，這個視窗無法替它開啟命令列。"
        } else {
            "The Better Monitor service decides what it collects, so this window can't turn command lines on for it."
        }
        .to_string();
    }
    if z {
        "系統監控沒有回應，請稍後再試。"
    } else {
        "Better Monitor didn't answer. Try again in a moment."
    }
    .to_string()
}

/// A collector, by what it measures.
pub(crate) fn collector_name(collector: &str, locale: Locale) -> String {
    let z = zh(locale);
    match collector {
        "linux.cpu" => {
            if z {
                "處理器"
            } else {
                "Processor"
            }
        }
        "linux.memory" => {
            if z {
                "記憶體"
            } else {
                "Memory"
            }
        }
        "linux.storage" => {
            if z {
                "磁碟"
            } else {
                "Disks"
            }
        }
        "linux.network" => {
            if z {
                "網路"
            } else {
                "Network"
            }
        }
        "linux.pressure" => {
            if z {
                "等待時間"
            } else {
                "Waiting time"
            }
        }
        "linux.process" => {
            if z {
                "處理程序"
            } else {
                "Processes"
            }
        }
        "linux.process.usage" => {
            if z {
                "各程式的網路與顯示卡用量"
            } else {
                "Network and graphics use by program"
            }
        }
        "linux.gpu" => {
            if z {
                "顯示卡"
            } else {
                "Graphics cards"
            }
        }
        "linux.npu" => "NPU",
        "linux.battery" => {
            if z {
                "電池與電源"
            } else {
                "Battery and power"
            }
        }
        "linux.sensors" => {
            if z {
                "溫度、風扇與感測器"
            } else {
                "Temperatures, fans, and sensors"
            }
        }
        _ => {
            if z {
                "其他來源"
            } else {
                "Other source"
            }
        }
    }
    .to_string()
}

pub(crate) fn coverage_sentence(
    measured: usize,
    missing: usize,
    unsupported: usize,
    denied: usize,
    locale: Locale,
) -> String {
    if zh(locale) {
        format!(
            "測量到 {measured} 項，{missing} 項暫時沒有數據，{unsupported} 項這台電腦不支援，{denied} 項需要權限。"
        )
    } else {
        format!(
            "{measured} measured, {missing} not available right now, {unsupported} not supported on this computer, {denied} need permission."
        )
    }
}

pub(crate) fn evidence_kind_word(kind: EvidenceKind, locale: Locale) -> &'static str {
    let z = zh(locale);
    match kind {
        EvidenceKind::SystemdUnit => {
            if z {
                "同一個系統服務"
            } else {
                "The same system service"
            }
        }
        EvidenceKind::Flatpak => {
            if z {
                "同一個 Flatpak 應用程式"
            } else {
                "The same Flatpak app"
            }
        }
        EvidenceKind::Snap => {
            if z {
                "同一個 Snap"
            } else {
                "The same Snap"
            }
        }
        EvidenceKind::DesktopApplication => {
            if z {
                "桌面啟動的同一個應用程式"
            } else {
                "One app as the desktop started it"
            }
        }
        EvidenceKind::Ancestry => {
            if z {
                "由誰啟動"
            } else {
                "Which process started it"
            }
        }
        EvidenceKind::ExecutableIdentity => {
            if z {
                "程式本身"
            } else {
                "The program itself"
            }
        }
    }
}

pub(crate) fn evidence_kind_explanation(kind: EvidenceKind, locale: Locale) -> &'static str {
    let z = zh(locale);
    match kind {
        EvidenceKind::SystemdUnit => {
            if z {
                "系統替每個服務記錄了哪些處理程序屬於它。"
            } else {
                "The system records which processes belong to each service."
            }
        }
        EvidenceKind::Flatpak | EvidenceKind::Snap => {
            if z {
                "封裝好的應用程式會在自己的範圍裡執行。"
            } else {
                "Packaged apps run in a space of their own."
            }
        }
        EvidenceKind::DesktopApplication => {
            if z {
                "桌面啟動應用程式時，會把它的處理程序放在一起。"
            } else {
                "When the desktop opens an app, it keeps that app's processes together."
            }
        }
        EvidenceKind::Ancestry => {
            if z {
                "沒有其他線索時，才把處理程序歸到啟動它的那一個底下。"
            } else {
                "Only when nothing else applies, a process joins the one that started it."
            }
        }
        EvidenceKind::ExecutableIdentity => {
            if z {
                "只用來命名，不用來合併。"
            } else {
                "Used for the name only, never to merge."
            }
        }
    }
}

pub(crate) fn scope_word(scope: Scope, locale: Locale) -> &'static str {
    let z = zh(locale);
    match scope {
        Scope::Yours => {
            if z {
                "只有你"
            } else {
                "Only you"
            }
        }
        Scope::Everyone => {
            if z {
                "所有人"
            } else {
                "Everyone"
            }
        }
        Scope::EveryoneChangedByYou => {
            if z {
                "所有人（已修改）"
            } else {
                "Everyone (edited)"
            }
        }
    }
}

/// Whether a service starts at login, in words, with how it is drawn.
pub(crate) fn startup_word(service: &ServiceUnit, locale: Locale) -> (&'static str, CellTone) {
    let z = zh(locale);
    if service.lock() == Some(ServiceLock::BetterOs) {
        return (
            if z {
                "由元件管理器控制"
            } else {
                "By Better Manager"
            },
            CellTone::Secondary,
        );
    }
    match service.startup {
        StartupState::Enabled => (if z { "會啟動" } else { "Starts" }, CellTone::Primary),
        StartupState::Disabled => (
            if z { "不會啟動" } else { "Doesn't start" },
            CellTone::Secondary,
        ),
        StartupState::OnDemand => (
            if z { "需要時啟動" } else { "When needed" },
            CellTone::Secondary,
        ),
        StartupState::Masked => (if z { "已封鎖" } else { "Blocked" }, CellTone::Caution),
        StartupState::Temporary => (
            if z {
                "只限這次登入"
            } else {
                "This session only"
            },
            CellTone::Secondary,
        ),
        StartupState::Unknown => (if z { "未知" } else { "Unknown" }, CellTone::Secondary),
    }
}

pub(crate) fn activity_word(activity: Activity, locale: Locale) -> (&'static str, CellTone) {
    let z = zh(locale);
    match activity {
        Activity::Running => (if z { "執行中" } else { "Running" }, CellTone::Primary),
        Activity::Starting => (if z { "正在啟動" } else { "Starting" }, CellTone::Primary),
        Activity::Stopping => (if z { "正在停止" } else { "Stopping" }, CellTone::Secondary),
        Activity::Stopped => (if z { "已停止" } else { "Stopped" }, CellTone::Secondary),
        Activity::Failed => (if z { "失敗" } else { "Failed" }, CellTone::Critical),
        Activity::Finished => (if z { "已完成" } else { "Finished" }, CellTone::Secondary),
    }
}

pub(crate) fn lock_sentence(lock: ServiceLock, locale: Locale) -> &'static str {
    let z = zh(locale);
    match lock {
        ServiceLock::BetterOs => {
            if z {
                "元件管理器會隨元件一起開啟或關閉它。"
            } else {
                "Better Manager turns this on and off with its component."
            }
        }
        ServiceLock::OnDemand => {
            if z {
                "其他程式需要它時才會啟動，沒有可以開關的設定。"
            } else {
                "It starts when another program needs it, so there is nothing to turn on or off."
            }
        }
        ServiceLock::Masked => {
            if z {
                "它被刻意封鎖，無法啟動。"
            } else {
                "It has been blocked on purpose and can't start."
            }
        }
        ServiceLock::Temporary => {
            if z {
                "這是某個程式為這次登入建立的。"
            } else {
                "A program created it for this session only."
            }
        }
        ServiceLock::Unknown => {
            if z {
                "讀不到它的啟動設定。"
            } else {
                "Its startup setting couldn't be read."
            }
        }
    }
}

/// How long ago something happened.
pub(crate) fn ago(now_ms: u64, at_ms: u64, locale: Locale) -> String {
    let z = zh(locale);
    if now_ms == 0 || at_ms > now_ms {
        return if z { "剛剛" } else { "just now" }.to_string();
    }
    let seconds = (now_ms - at_ms) / 1000;
    let (count, en, zh_unit) = match seconds {
        0..=59 => return if z { "剛剛" } else { "just now" }.to_string(),
        60..=3_599 => (seconds / 60, "minute", "分鐘"),
        3_600..=86_399 => (seconds / 3_600, "hour", "小時"),
        _ => (seconds / 86_400, "day", "天"),
    };
    if z {
        format!("{count} {zh_unit}前")
    } else if count == 1 {
        format!("1 {en} ago")
    } else {
        format!("{count} {en}s ago")
    }
}

/// The title of an incident a person marked by hand with a version from
/// before slowdowns were recorded automatically.
pub(crate) fn marked_title(when: &str, locale: Locale) -> String {
    if zh(locale) {
        format!("{when}手動標記")
    } else {
        format!("Marked by hand {when}")
    }
}

/// What counts as a slowdown: one row per kind the service detects, each a
/// short name and the rule, and a last row on how records are kept. Written
/// from the rules themselves, so the page can't drift from what the service
/// does.
pub(crate) fn slowdown_rules(rules: &SlowdownRules, locale: Locale) -> Vec<(String, String)> {
    let onset = spelled_duration(rules.onset_seconds, locale);
    let cpu_onset = spelled_duration(rules.cpu_onset_seconds, locale);
    let stall = spelled_duration(rules.stall_seconds, locale);
    let quiet = spelled_duration(rules.quiet_seconds, locale);
    let percent = |value: f64| format!("{value:.0}%");
    let (ms, mf) = (
        percent(rules.memory_some_percent),
        percent(rules.memory_full_percent),
    );
    let df = percent(rules.disk_full_percent);
    let cs = percent(rules.cpu_some_percent);
    let load = format!("{:.0}", rules.cpu_load_per_processor);
    let free = percent(rules.memory_available_percent);
    let cap = rules.max_per_day;
    let rows: [(&str, String); 8] = if zh(locale) {
        [
            (
                "程式在等記憶體",
                format!(
                    "持續 {onset}以上：有程式至少 {ms} 的時間在等，或所有程式同時在等的時間至少 {mf}。"
                ),
            ),
            (
                "所有程式都在等磁碟",
                format!(
                    "有事要做的程式全都同時在等磁碟，至少 {df} 的時間，持續 {onset}以上。只有一個大檔案複製自己在等的話不算。"
                ),
            ),
            (
                "程式在排隊等處理器",
                format!(
                    "至少 {cs} 的時間在排隊，而且等著執行的程式超過處理器數量的 {load} 倍，持續 {cpu_onset}以上。把每個核心都用滿的編譯不算。"
                ),
            ),
            (
                "記憶體快用完",
                format!("可用的記憶體不到 {free}，電腦又在使用置換空間，持續 {onset}以上。"),
            ),
            (
                "程式因記憶體不夠被關掉",
                "核心因為記憶體不夠強制關掉程式的那一刻就會記下來。如果是 Ubuntu 的 systemd-oomd 關掉的，這裡看不到，只會記成在等記憶體。"
                    .to_string(),
            ),
            (
                "處理器太熱而降速",
                format!(
                    "處理器一直自動降速，持續 {onset}以上。只有會回報這件事的處理器（大多是 Intel）才看得到。"
                ),
            ),
            (
                "電腦卡住",
                format!("電腦沒在睡眠，系統監控每秒一次的測量卻晚了 {stall}以上。"),
            ),
            (
                "一次變慢只記一筆",
                format!(
                    "只維持幾秒的尖峰不會記錄。不管變慢多久都只記一筆，要連續 {quiet}都沒有上述情況才算結束。一天最多記 {cap} 筆。"
                ),
            ),
        ]
    } else {
        [
            (
                "Programs waiting for memory",
                format!(
                    "For {onset} or more: some of them at least {ms} of the time, or all of them at once at least {mf}."
                ),
            ),
            (
                "Everything waiting for the disk",
                format!(
                    "Every program with work to do waiting for the disk at once, at least {df} of the time, for {onset} or more. One large copy that keeps only itself waiting does not count."
                ),
            ),
            (
                "Programs waiting for the processor",
                format!(
                    "Waiting for a turn at least {cs} of the time, with more than {load} times as many programs ready to run as there are processors, for {cpu_onset} or more. A build that keeps every core busy does not count."
                ),
            ),
            (
                "Memory nearly gone",
                format!(
                    "Less than {free} of memory available while the computer is using swap space, for {onset} or more."
                ),
            ),
            (
                "A program closed because memory ran out",
                "Written down the moment the kernel closes a program because memory ran out. When Ubuntu's systemd-oomd closes one instead, it is not seen here; it shows as waiting for memory."
                    .to_string(),
            ),
            (
                "The processor slowed down by heat",
                format!(
                    "The processor keeps slowing itself down for {onset} or more. Only processors that report this (most Intel ones) can be watched."
                ),
            ),
            (
                "The computer froze",
                format!(
                    "Better Monitor's own once-a-second reading came {stall} or more late while the computer was awake."
                ),
            ),
            (
                "One slowdown, one record",
                format!(
                    "A spike of a few seconds is not recorded. A slowdown is one record however long it lasts, and it is over only after {quiet} without any of these. At most {cap} are recorded a day."
                ),
            ),
        ]
    };
    rows.into_iter()
        .map(|(name, rule)| (name.to_string(), rule))
        .collect()
}

/// A length of time in words, for a sentence: "10 seconds", "1 minute".
fn spelled_duration(value: u64, locale: Locale) -> String {
    let minutes = value / 60;
    match (zh(locale), value.is_multiple_of(60) && value > 0) {
        (true, true) => format!("{minutes} 分鐘"),
        (true, false) => format!("{value} 秒"),
        (false, true) if minutes == 1 => "1 minute".to_string(),
        (false, true) => format!("{minutes} minutes"),
        (false, false) if value == 1 => "1 second".to_string(),
        (false, false) => format!("{value} seconds"),
    }
}

/// What a detected slowdown was, as the title of its card.
pub(crate) fn slowdown_title(cause: Option<&SlowdownCause>, locale: Locale) -> String {
    let z = zh(locale);
    let kind = cause.map(|cause| cause.kind).unwrap_or(SlowdownKind::Other);
    let count = cause.map(|cause| cause.count).unwrap_or(0);
    let fixed = |chinese: &str, english: &str| if z { chinese } else { english }.to_string();
    match kind {
        SlowdownKind::MemoryPressure => fixed("程式在等記憶體", "Programs were waiting for memory"),
        SlowdownKind::DiskPressure => {
            fixed("所有程式都在等磁碟", "Everything was waiting for the disk")
        }
        SlowdownKind::CpuPressure => fixed(
            "程式在排隊等處理器",
            "Programs were waiting for the processor",
        ),
        SlowdownKind::MemoryExhausted => fixed("記憶體快用完了", "Memory nearly ran out"),
        SlowdownKind::OutOfMemoryKill => match (z, count) {
            (true, 0 | 1) => "記憶體不夠，有程式被強制關掉".to_string(),
            (true, n) => format!("記憶體不夠，有 {n} 個程式被強制關掉"),
            (false, 0 | 1) => "A program was closed because memory ran out".to_string(),
            (false, n) => format!("{n} programs were closed because memory ran out"),
        },
        SlowdownKind::ThermalThrottling => fixed(
            "處理器太熱而降速",
            "The processor slowed down because it was too hot",
        ),
        SlowdownKind::Stalled => fixed("電腦卡住了", "The computer froze"),
        SlowdownKind::Other => fixed("有東西讓電腦變慢", "Something slowed the computer down"),
    }
}

/// When a detected slowdown began and how long it lasted, or how long it
/// has lasted so far, counted to now.
pub(crate) fn slowdown_when(now_ms: u64, detection: &Detection, locale: Locale) -> String {
    let when = ago(now_ms, detection.started_at_unix_ms, locale);
    let lasted = if detection.ongoing {
        detection
            .duration_seconds()
            .max(now_ms.saturating_sub(detection.started_at_unix_ms) / 1_000)
    } else {
        detection.duration_seconds()
    };
    let length = seconds(lasted, locale);
    match (zh(locale), detection.ongoing, lasted) {
        (true, true, _) => format!("{when}開始 · 還在持續，已經 {length}"),
        (false, true, _) => format!("Began {when} · still going, {length} so far"),
        (true, false, 0) => when,
        (false, false, 0) => capitalized(&when),
        (true, false, _) => format!("{when} · 持續 {length}"),
        (false, false, _) => format!("{} · lasted {length}", capitalized(&when)),
    }
}

fn capitalized(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// The memory pressure at which Ubuntu's systemd-oomd may close programs on
/// its own, and for how long. Its default for a person's session is a limit
/// of 50% held for 20 seconds; this is a reading of that default, not of the
/// machine's configuration, which is why the sentence says "may".
const OOMD_PRESSURE_PERCENT: f64 = 50.0;
const OOMD_PRESSURE_SECONDS: u64 = 20;

/// How bad the leading cause got, in a sentence. `None` where the title has
/// already said everything there is.
pub(crate) fn slowdown_severity(cause: &SlowdownCause, locale: Locale) -> Option<String> {
    let z = zh(locale);
    match cause.kind {
        SlowdownKind::MemoryPressure | SlowdownKind::CpuPressure => {
            let worst = format!("{:.0}%", cause.worst);
            let mut sentence = if z {
                format!("最嚴重的時候，程式有 {worst} 的時間在等。")
            } else {
                format!("At worst, programs were waiting {worst} of the time.")
            };
            // systemd-oomd ends programs without the kernel's counter going
            // up, so this record cannot see it; it can say when it may have.
            if cause.kind == SlowdownKind::MemoryPressure
                && cause.worst >= OOMD_PRESSURE_PERCENT
                && cause.seconds >= OOMD_PRESSURE_SECONDS
            {
                sentence.push_str(if z {
                    "壓力這麼高又這麼久時，Ubuntu 的 systemd-oomd 可能會自己關掉程式，這筆紀錄看不到那件事。"
                } else {
                    " Pressure this high for this long is when Ubuntu's systemd-oomd may close programs on its own, which this record cannot see."
                });
            }
            Some(sentence)
        }
        SlowdownKind::DiskPressure => {
            let worst = format!("{:.0}%", cause.worst);
            Some(if z {
                format!("最嚴重的時候，所有程式有 {worst} 的時間同時在等。")
            } else {
                format!("At worst, everything was waiting {worst} of the time.")
            })
        }
        SlowdownKind::MemoryExhausted => {
            let worst = format!("{:.1}%", cause.worst);
            Some(if z {
                format!("最嚴重的時候只剩 {worst} 的記憶體可用，電腦還在使用置換空間。")
            } else {
                format!(
                    "At worst only {worst} of memory was available, and the computer was using swap space."
                )
            })
        }
        SlowdownKind::ThermalThrottling if cause.count > 0 => Some(if z {
            format!("處理器回報降速 {} 次。", cause.count)
        } else if cause.count == 1 {
            "The processor reported slowing down once.".to_string()
        } else {
            format!("The processor reported slowing down {} times.", cause.count)
        }),
        SlowdownKind::Stalled => {
            let longest = seconds(cause.worst.round() as u64, locale);
            Some(match (z, cause.count) {
                (true, 0 | 1) => format!("系統監控自己也有 {longest}量不到任何數據。"),
                (true, n) => {
                    format!("卡住 {n} 次，最久的一次系統監控自己也有 {longest}量不到任何數據。")
                }
                (false, 0 | 1) => {
                    format!("Better Monitor itself could not take a reading for {longest}.")
                }
                (false, n) => format!(
                    "It froze {n} times; at the longest, Better Monitor itself could not take a reading for {longest}."
                ),
            })
        }
        _ => None,
    }
}

/// Another cause that held during the same slowdown, in a line.
pub(crate) fn slowdown_also(cause: &SlowdownCause, locale: Locale) -> String {
    let z = zh(locale);
    let what = match (cause.kind, z) {
        (SlowdownKind::MemoryPressure, true) => "等記憶體",
        (SlowdownKind::MemoryPressure, false) => "waiting for memory",
        (SlowdownKind::DiskPressure, true) => "所有程式等磁碟",
        (SlowdownKind::DiskPressure, false) => "everything waiting for the disk",
        (SlowdownKind::CpuPressure, true) => "等處理器",
        (SlowdownKind::CpuPressure, false) => "waiting for the processor",
        (SlowdownKind::MemoryExhausted, true) => "記憶體快用完",
        (SlowdownKind::MemoryExhausted, false) => "memory nearly gone",
        (SlowdownKind::OutOfMemoryKill, true) => "程式因記憶體不夠被關掉",
        (SlowdownKind::OutOfMemoryKill, false) => "a program closed because memory ran out",
        (SlowdownKind::ThermalThrottling, true) => "處理器太熱降速",
        (SlowdownKind::ThermalThrottling, false) => "the processor slowed down by heat",
        (SlowdownKind::Stalled, true) => "電腦卡住",
        (SlowdownKind::Stalled, false) => "the computer frozen",
        (SlowdownKind::Other, true) => "其他原因",
        (SlowdownKind::Other, false) => "something else",
    };
    let length = seconds(cause.seconds, locale);
    match (z, cause.seconds) {
        (true, 0) => format!("同時：{what}"),
        (false, 0) => format!("Also: {what}"),
        (true, _) => format!("同時：{what}（{length}）"),
        (false, _) => format!("Also: {what} ({length})"),
    }
}

/// The line on the Events page when the daily cap kept slowdowns out.
pub(crate) fn skipped_sentence(count: u64, cap: usize, locale: Locale) -> String {
    match (zh(locale), count) {
        (true, n) => {
            format!("已達一天最多記 {cap} 筆的上限：過去 24 小時另有 {n} 次變慢沒有記錄。")
        }
        (false, 1) => format!(
            "The limit of {cap} a day was reached: 1 more slowdown in the last 24 hours was not recorded."
        ),
        (false, n) => format!(
            "The limit of {cap} a day was reached: {n} more slowdowns in the last 24 hours were not recorded."
        ),
    }
}

/// The heading over a detected slowdown's process list.
pub(crate) fn slowdown_processes_heading(
    resource: SlowdownResource,
    locale: Locale,
) -> &'static str {
    match (resource, zh(locale)) {
        (SlowdownResource::Processor, true) => "當時用最多處理器的程式",
        (SlowdownResource::Processor, false) => "Using the most processor time then",
        (SlowdownResource::Memory, true) => "當時用最多記憶體的程式",
        (SlowdownResource::Memory, false) => "Using the most memory then",
        (SlowdownResource::Disk, true) => "當時讀寫磁碟最多的程式",
        (SlowdownResource::Disk, false) => "Reading and writing the most then",
        (SlowdownResource::Other, true) => "當時最忙的程式",
        (SlowdownResource::Other, false) => "Busiest programs then",
    }
}

pub(crate) fn window_sentence(before: u64, after: u64, locale: Locale) -> String {
    let (b, a) = (seconds(before, locale), seconds(after, locale));
    if zh(locale) {
        format!("涵蓋前 {b} 到後 {a}")
    } else {
        format!("Covers {b} before to {a} after")
    }
}

/// What the history page says about saved records the store could not read
/// and kept aside instead of throwing away.
pub(crate) fn set_aside_sentence(count: u64, locale: Locale) -> String {
    if zh(locale) {
        format!(
            "有 {count} 筆紀錄的格式這個版本看不懂，所以沒有顯示。原始內容已另外保存，不會被刪掉，之後的版本可能讀得出來。"
        )
    } else if count == 1 {
        "1 record is in a form this version cannot read, so it is not shown. It is kept aside unchanged, so a later version may still read it.".to_string()
    } else {
        format!(
            "{count} records are in a form this version cannot read, so they are not shown. They are kept aside unchanged, so a later version may still read them."
        )
    }
}

pub(crate) fn gap_sentence(reason: &str, duration_ms: u64, locale: Locale) -> String {
    format!("{reason} · {}", seconds(duration_ms / 1000, locale))
}

/// A system reading's name and how to write its value, for the readings an
/// incident compares. `None` for a reading the window has no words for.
/// How a value is written.
type Formatter = fn(f64) -> String;

fn metric_words(id: &str, locale: Locale) -> Option<(&'static str, Formatter)> {
    let z = zh(locale);
    let percent: fn(f64) -> String = |v| crate::tables::share(v);
    let bytes: fn(f64) -> String = crate::tables::bytes;
    let number: fn(f64) -> String = |v| format!("{v:.2}");
    let per_second: fn(f64) -> String = |v| format!("{v:.0}/s");
    Some(match id {
        "cpu.utilization.busy" => (
            if z {
                "處理器使用率"
            } else {
                "Processor use"
            },
            percent,
        ),
        "cpu.utilization.user" => (
            if z {
                "程式使用處理器"
            } else {
                "Processor use by programs"
            },
            percent,
        ),
        "cpu.utilization.iowait" => (
            if z {
                "處理器等候磁碟"
            } else {
                "Processor waiting for disk"
            },
            percent,
        ),
        "cpu.load.average.1m" => (
            if z {
                "等待執行（1 分鐘）"
            } else {
                "Waiting to run (1 minute)"
            },
            number,
        ),
        "cpu.load.average.5m" => (
            if z {
                "等待執行（5 分鐘）"
            } else {
                "Waiting to run (5 minutes)"
            },
            number,
        ),
        "cpu.load.average.15m" => (
            if z {
                "等待執行（15 分鐘）"
            } else {
                "Waiting to run (15 minutes)"
            },
            number,
        ),
        "cpu.tasks.runnable" => (
            if z {
                "準備執行的工作"
            } else {
                "Tasks ready to run"
            },
            number,
        ),
        "cpu.tasks.blocked" => (
            if z {
                "等候磁碟的工作"
            } else {
                "Tasks waiting for disk"
            },
            number,
        ),
        "cpu.context_switches.rate" => (
            if z {
                "工作切換次數"
            } else {
                "Task switches"
            },
            per_second,
        ),
        "cpu.interrupts.rate" => (
            if z {
                "硬體中斷次數"
            } else {
                "Hardware interrupts"
            },
            per_second,
        ),
        "cpu.processes.created.rate" => (
            if z {
                "新啟動的處理程序"
            } else {
                "Processes started"
            },
            per_second,
        ),
        "memory.available" => (
            if z {
                "可用記憶體"
            } else {
                "Available memory"
            },
            bytes,
        ),
        "memory.used" => (
            if z {
                "使用中的記憶體"
            } else {
                "Memory in use"
            },
            bytes,
        ),
        "memory.free" => (if z { "閒置記憶體" } else { "Free memory" }, bytes),
        "memory.cached" => (
            if z {
                "快取記憶體"
            } else {
                "Cached memory"
            },
            bytes,
        ),
        "memory.dirty" => (
            if z {
                "等待寫入的記憶體"
            } else {
                "Memory waiting to be saved"
            },
            bytes,
        ),
        "memory.swap.used" => (
            if z {
                "使用中的置換空間"
            } else {
                "Swap in use"
            },
            bytes,
        ),
        "memory.utilization" => (
            if z {
                "記憶體使用率"
            } else {
                "Memory use"
            },
            percent,
        ),
        "memory.fault.major.rate" => (
            if z {
                "從磁碟補回記憶體"
            } else {
                "Memory read back from disk"
            },
            per_second,
        ),
        "memory.swap_in.rate" => (
            if z {
                "從置換空間讀回"
            } else {
                "Read back from swap"
            },
            per_second,
        ),
        "memory.swap_out.rate" => (
            if z {
                "寫到置換空間"
            } else {
                "Written to swap"
            },
            per_second,
        ),
        _ => return None,
    })
}

/// One reading an incident compares: its name, and "before → at the mark".
pub(crate) fn shift_sentence(
    id: &str,
    baseline: f64,
    at_marker: f64,
    locale: Locale,
) -> Option<(String, String)> {
    let (name, format) = metric_words(id, locale)?;
    Some((
        name.to_string(),
        format!("{} → {}", format(baseline), format(at_marker)),
    ))
}

pub(crate) fn inventory_value(inventory: Option<&Inventory>, key: &str) -> Option<String> {
    inventory?
        .entries
        .get(key)
        .map(|entry| entry.value.clone())
        .filter(|value| !value.is_empty())
}

/// Every graphics card the inventory names, with its driver.
pub(crate) fn graphics_cards(
    inventory: Option<&Inventory>,
    locale: Locale,
) -> Vec<(String, String)> {
    let Some(inventory) = inventory else {
        return Vec::new();
    };
    let z = zh(locale);
    let mut cards: Vec<String> = inventory
        .entries
        .keys()
        .filter_map(|key| {
            let rest = key.strip_prefix("graphics.")?;
            let (card, _) = rest.rsplit_once('.')?;
            Some(card.to_string())
        })
        .collect();
    cards.sort();
    cards.dedup();
    cards
        .into_iter()
        .map(|card| {
            let driver = inventory_value(Some(inventory), &format!("graphics.{card}.driver"));
            let device = inventory_value(Some(inventory), &format!("graphics.{card}.pci_id"));
            let value = match (driver, device, z) {
                (Some(d), Some(id), false) => format!("Driver {d} · device {id}"),
                (Some(d), Some(id), true) => format!("驅動程式 {d} · 裝置 {id}"),
                (Some(d), None, false) => format!("Driver {d}"),
                (Some(d), None, true) => format!("驅動程式 {d}"),
                (None, Some(id), false) => format!("Device {id}"),
                (None, Some(id), true) => format!("裝置 {id}"),
                (None, None, _) => copy(locale).not_available.to_string(),
            };
            (card, value)
        })
        .collect()
}

/// An inventory key as a section and a name, with how to write its value.
/// `None` for a key the window has no words for; it is left out rather than
/// shown as itself.
pub(crate) fn inventory_label(
    key: &str,
    value: &str,
    locale: Locale,
) -> Option<(usize, String, String)> {
    let z = zh(locale);
    let t = |en: &str, zh_text: &str| {
        if z {
            zh_text.to_string()
        } else {
            en.to_string()
        }
    };
    let size = |value: &str| {
        value
            .parse::<u64>()
            .map(words::bytes)
            .unwrap_or_else(|_| value.to_string())
    };
    let fixed = match key {
        "os.name" => Some((0, t("Name", "名稱"), value.to_string())),
        "os.version" => Some((0, t("Version", "版本"), value.to_string())),
        "os.variant" => Some((0, t("Edition", "版本類型"), value.to_string())),
        "kernel.release" => Some((0, t("Kernel", "核心"), value.to_string())),
        "kernel.version" => Some((0, t("Kernel build", "核心建置"), value.to_string())),
        "host.name" => Some((1, t("Computer name", "電腦名稱"), value.to_string())),
        "session.user" => Some((1, t("User", "使用者"), value.to_string())),
        "session.home" => Some((1, t("Home folder", "個人資料夾"), value.to_string())),
        "session.desktop" => Some((1, t("Desktop", "桌面環境"), value.to_string())),
        "session.type" => Some((1, t("Session type", "工作階段類型"), value.to_string())),
        "session.display_protocol" => Some((1, t("Display system", "顯示系統"), value.to_string())),
        "cpu.model" => Some((2, t("Processor", "處理器"), value.to_string())),
        "cpu.logical_count" => Some((2, t("Logical processors", "邏輯處理器"), value.to_string())),
        "memory.total_bytes" => Some((2, t("Installed memory", "記憶體"), size(value))),
        _ => None,
    };
    if fixed.is_some() {
        return fixed;
    }
    let (prefix, rest) = key.split_once('.')?;
    let (item, field) = rest.rsplit_once('.')?;
    match (prefix, field) {
        ("graphics", "driver") => Some((
            2,
            format!("{item} {}", t("driver", "驅動程式")),
            value.to_string(),
        )),
        ("graphics", "pci_id") => Some((
            2,
            format!("{item} {}", t("device", "裝置")),
            value.to_string(),
        )),
        ("storage", "model") => Some((
            3,
            format!("{item} {}", t("model", "型號")),
            value.to_string(),
        )),
        ("storage", "size_bytes") => {
            Some((3, format!("{item} {}", t("capacity", "容量")), size(value)))
        }
        ("storage", "serial") => Some((
            3,
            format!("{item} {}", t("serial number", "序號")),
            value.to_string(),
        )),
        ("storage", "rotational") => Some((
            3,
            format!("{item} {}", t("type", "類型")),
            if value == "1" || value == "true" {
                copy(locale).hdd.to_string()
            } else {
                copy(locale).ssd.to_string()
            },
        )),
        ("network", "mac") => Some((
            4,
            format!("{item} {}", t("hardware address", "硬體位址")),
            value.to_string(),
        )),
        ("network", "state") => Some((
            4,
            format!("{item} {}", t("status", "狀態")),
            match value {
                "up" => adapter_status(Some(true), locale).to_string(),
                "down" => adapter_status(Some(false), locale).to_string(),
                _ => adapter_status(None, locale).to_string(),
            },
        )),
        ("betteros", "version") => {
            let component = item.strip_prefix("component.")?;
            Some((5, component.to_string(), value.to_string()))
        }
        _ if key.starts_with("filesystem") => {
            let (target, field) = key.strip_prefix("filesystem")?.rsplit_once('.')?;
            let target = if target.is_empty() || target == "." {
                "/".to_string()
            } else {
                target.replace('.', "/")
            };
            match field {
                "source" => Some((
                    6,
                    format!("{target} {}", t("device", "裝置")),
                    value.to_string(),
                )),
                "type" => Some((
                    6,
                    format!("{target} {}", t("file system", "檔案系統")),
                    value.to_string(),
                )),
                _ => None,
            }
        }
        _ => None,
    }
}

fn inventory_section_title(index: usize, locale: Locale) -> String {
    let z = zh(locale);
    match index {
        0 => {
            if z {
                "作業系統"
            } else {
                "Operating system"
            }
        }
        1 => {
            if z {
                "工作階段"
            } else {
                "Session"
            }
        }
        2 => {
            if z {
                "硬體"
            } else {
                "Hardware"
            }
        }
        3 => {
            if z {
                "磁碟"
            } else {
                "Disks"
            }
        }
        4 => {
            if z {
                "網路"
            } else {
                "Network"
            }
        }
        5 => {
            if z {
                "Better OS 元件"
            } else {
                "Better OS components"
            }
        }
        _ => {
            if z {
                "檔案系統"
            } else {
                "File systems"
            }
        }
    }
    .to_string()
}

/// One line of System information: its name, its value, and whether it may
/// leave this computer.
pub(crate) type InventoryRow = (String, String, Sensitivity);

/// The inventory as titled sections of named values, in a fixed order.
pub(crate) fn inventory_sections(
    inventory: &Inventory,
    locale: Locale,
) -> Vec<(String, Vec<InventoryRow>)> {
    // The fixed keys in the order a person reads them; device rows follow,
    // grouped by device as the keys sort.
    const ORDER: [&str; 14] = [
        "os.name",
        "os.version",
        "os.variant",
        "kernel.release",
        "kernel.version",
        "host.name",
        "session.user",
        "session.home",
        "session.desktop",
        "session.type",
        "session.display_protocol",
        "cpu.model",
        "cpu.logical_count",
        "memory.total_bytes",
    ];
    // Rank, key, name, value, sensitivity.
    type Ranked = (usize, String, String, String, Sensitivity);
    let mut ranked: Vec<Vec<Ranked>> = vec![Vec::new(); 7];
    for (key, entry) in &inventory.entries {
        if let Some((section, name, value)) = inventory_label(key, &entry.value, locale) {
            let rank = ORDER.iter().position(|k| k == key).unwrap_or(ORDER.len());
            ranked[section].push((rank, key.clone(), name, value, entry.sensitivity));
        }
    }
    for rows in &mut ranked {
        rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    }
    ranked
        .into_iter()
        .enumerate()
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(index, rows)| {
            (
                inventory_section_title(index, locale),
                rows.into_iter()
                    .map(|(_, _, name, value, sensitivity)| (name, value, sensitivity))
                    .collect(),
            )
        })
        .collect()
}

/// What changed between two looks at the machine, in words.
pub(crate) fn inventory_changes(diff: &InventoryDiff, locale: Locale) -> Vec<(String, String)> {
    let z = zh(locale);
    let none = if z { "沒有" } else { "nothing" };
    let mut changes = Vec::new();
    for (list, verb_en, verb_zh) in [
        (&diff.changed, "Changed", "變更"),
        (&diff.added, "Added", "新增"),
        (&diff.removed, "Removed", "移除"),
    ] {
        for change in list {
            let shown = change
                .after
                .as_deref()
                .or(change.before.as_deref())
                .unwrap_or("");
            let Some((_, name, _)) = inventory_label(&change.key, shown, locale) else {
                continue;
            };
            let write = |value: Option<&str>| {
                value
                    .and_then(|value| inventory_label(&change.key, value, locale))
                    .map(|(_, _, value)| value)
                    .unwrap_or_else(|| none.to_string())
            };
            let before = write(change.before.as_deref());
            let after = write(change.after.as_deref());
            changes.push((
                format!("{} · {name}", if z { verb_zh } else { verb_en }),
                format!("{before} → {after}"),
            ));
        }
    }
    changes
}

pub(crate) fn captures_sentence(captures: u32, locale: Locale) -> String {
    if zh(locale) {
        format!("系統監控查看過這台電腦 {captures} 次。標示「只留在這台電腦」的值不會匯出。")
    } else {
        let times = match captures {
            1 => "once".to_string(),
            2 => "twice".to_string(),
            n => format!("{n} times"),
        };
        format!(
            "Better Monitor has looked at this computer {times}. Values marked \"stays on this computer\" are never exported."
        )
    }
}

/// A graphics card as a person names it: the driver's product name when it
/// gives one, otherwise the vendor and the card (`AMD graphics (card1)`).
///
/// Integrated graphics have no product name of their own in sysfs, but the
/// processor's model often carries it (`AMD Ryzen AI 9 HX 370 w/ Radeon
/// 890M`), and that is what a person knows the graphics as.
pub(crate) fn gpu_title(
    gpu: &crate::charts::GpuReadings,
    cpu_model: Option<&str>,
    locale: Locale,
) -> String {
    if let Some(name) = &gpu.name {
        return name.clone();
    }
    if let (Some(vendor), Some(model)) = (gpu.vendor.as_deref(), cpu_model)
        && model.starts_with(vendor)
        && let Some((_, graphics)) = model.split_once(" w/ ")
    {
        return format!("{vendor} {}", graphics.trim());
    }
    let vendor = gpu.vendor.clone();
    let card = gpu.card.clone().unwrap_or_else(|| gpu.key.clone());
    match (vendor, zh(locale)) {
        (Some(vendor), false) => format!("{vendor} graphics ({card})"),
        (Some(vendor), true) => format!("{vendor} 顯示卡（{card}）"),
        (None, false) => format!("Graphics card ({card})"),
        (None, true) => format!("顯示卡（{card}）"),
    }
}

/// An NPU as a person names it.
pub(crate) fn npu_title(npu: &crate::charts::NpuReadings, locale: Locale) -> String {
    match (&npu.model, &npu.vendor, zh(locale)) {
        (Some(model), Some(vendor), _) => format!("{vendor} {model}"),
        (Some(model), None, _) => model.clone(),
        (None, Some(vendor), false) => format!("{vendor} NPU"),
        (None, Some(vendor), true) => format!("{vendor} NPU"),
        (None, None, _) => "NPU".to_string(),
    }
}

/// The NPU's power state in words: asleep or running.
pub(crate) fn npu_state(npu: &crate::charts::NpuReadings, locale: Locale) -> Option<&'static str> {
    let c = copy(locale);
    let asleep = npu.runtime_status.as_deref() == Some("suspended")
        || npu
            .power_state
            .as_deref()
            .is_some_and(|state| state.starts_with("D3"));
    let running =
        npu.runtime_status.as_deref() == Some("active") || npu.power_state.as_deref() == Some("D0");
    if asleep {
        Some(c.npu_asleep)
    } else if running {
        Some(c.npu_running)
    } else {
        None
    }
}

/// What a hwmon chip is, in words: `k10temp` is the processor.
pub(crate) fn chip_title(chip: &str, locale: Locale) -> String {
    let z = zh(locale);
    let lower = chip.to_ascii_lowercase();
    let word = |en: &'static str, zh_word: &'static str| -> String {
        if z {
            zh_word.to_string()
        } else {
            en.to_string()
        }
    };
    if ["k10temp", "coretemp", "zenpower", "cpu_thermal"].contains(&lower.as_str()) {
        word("Processor", "處理器")
    } else if ["amdgpu", "radeon", "i915", "xe", "nouveau", "nvidia"].contains(&lower.as_str()) {
        word("Graphics card", "顯示卡")
    } else if lower.starts_with("nvme") {
        word("SSD", "固態硬碟")
    } else if lower.starts_with("drivetemp") {
        word("Disk", "磁碟")
    } else if lower == "acpitz" {
        word("Motherboard (ACPI)", "主機板（ACPI）")
    } else if lower == "acpi_fan" {
        word("Fan (ACPI)", "風扇（ACPI）")
    } else if lower == "asus" || lower.starts_with("asus") {
        word("ASUS embedded controller", "ASUS 嵌入式控制器")
    } else if lower.starts_with("thinkpad") {
        word("ThinkPad embedded controller", "ThinkPad 嵌入式控制器")
    } else if lower.starts_with("dell") {
        word("Dell embedded controller", "Dell 嵌入式控制器")
    } else if lower.contains("phy") || lower.starts_with("iwlwifi") || lower.starts_with("ath") {
        word("Wi-Fi", "Wi-Fi")
    } else if lower.starts_with("bat") {
        word("Battery", "電池")
    } else if lower.starts_with("ucsi_source_psy") {
        word("USB-C port", "USB-C 連接埠")
    } else if lower.starts_with("acad") || lower.starts_with("ac") && lower.len() <= 4 {
        word("Power adapter", "電源轉接器")
    } else {
        chip.to_string()
    }
}

/// One hwmon reading's name: the chip in words, then the driver's label
/// when it gave one (`Processor · Tctl`).
pub(crate) fn sensor_title(sensor: &crate::charts::SensorReadings, locale: Locale) -> String {
    let chip = chip_title(&sensor.chip, locale);
    match sensor.label.as_deref() {
        // A label that is only the input's own name (`temp1`) says nothing.
        Some(label) if !label.starts_with(sensor_prefix(&sensor.kind)) || label.contains(' ') => {
            let label = match label {
                "cpu_fan" => {
                    if zh(locale) {
                        "處理器風扇"
                    } else {
                        "Processor fan"
                    }
                }
                "gpu_fan" => {
                    if zh(locale) {
                        "顯示卡風扇"
                    } else {
                        "Graphics fan"
                    }
                }
                other => other,
            };
            format!("{chip} · {label}")
        }
        _ => chip,
    }
}

fn sensor_prefix(kind: &str) -> &'static str {
    match kind {
        "temperature" => "temp",
        "fan" => "fan",
        "voltage" => "in",
        "current" => "curr",
        "power" => "power",
        "frequency" => "freq",
        _ => "\u{0}",
    }
}

/// A hwmon reading in its unit.
pub(crate) fn sensor_value(kind: &str, value: f64) -> String {
    match kind {
        "temperature" => format!("{value:.0} °C"),
        "fan" => format!("{} RPM", group_thousands(value.round() as u64)),
        "voltage" => format!("{value:.2} V"),
        "current" => format!("{value:.2} A"),
        "power" => crate::tables::watts(value),
        "frequency" => {
            if value >= 1e9 {
                format!("{:.2} GHz", value / 1e9)
            } else {
                format!("{:.0} MHz", value / 1e6)
            }
        }
        _ => format!("{value}"),
    }
}

fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A battery's state in words.
pub(crate) fn battery_status(status: Option<&str>, locale: Locale) -> &'static str {
    let c = copy(locale);
    match status {
        Some("charging") => c.battery_charging,
        Some("discharging") => c.battery_discharging,
        Some("full") => c.battery_full,
        Some("not_charging") => c.battery_not_charging,
        _ => c.battery_unknown,
    }
}

/// Hours and minutes: `2 h 10 min`, `2 小時 10 分鐘`.
pub(crate) fn hours_minutes(seconds: f64, locale: Locale) -> String {
    let minutes = (seconds / 60.0).round().max(0.0) as u64;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    match (hours, zh(locale)) {
        (0, false) => format!("{minutes} min"),
        (0, true) => format!("{minutes} 分鐘"),
        (h, false) => format!("{h} h {minutes} min"),
        (h, true) => format!("{h} 小時 {minutes} 分鐘"),
    }
}

/// The one line under a battery's figure: its state, and how long is left.
pub(crate) fn battery_line(battery: &crate::charts::BatteryReadings, locale: Locale) -> String {
    let status = battery_status(battery.status.as_deref(), locale);
    let z = zh(locale);
    match (
        battery.status.as_deref(),
        battery.time_remaining,
        battery.time_to_full,
    ) {
        (Some("discharging"), Some(left), _) => {
            let left = hours_minutes(left, locale);
            if z {
                format!("{status}，剩下約 {left}")
            } else {
                format!("{status}, about {left} left")
            }
        }
        (Some("charging"), _, Some(full)) => {
            let full = hours_minutes(full, locale);
            if z {
                format!("{status}，約 {full}後充滿")
            } else {
                format!("{status}, full in about {full}")
            }
        }
        _ => status.to_string(),
    }
}

/// Watt-hours: `32.4 Wh`.
pub(crate) fn watt_hours(value: f64) -> String {
    if value < 1.0 {
        format!("{:.0} mWh", value * 1000.0)
    } else {
        format!("{value:.1} Wh")
    }
}

/// What kind of power source, in words.
pub(crate) fn power_source_title(
    source: &crate::charts::PowerSourceReadings,
    locale: Locale,
) -> String {
    let c = copy(locale);
    match source.kind.as_deref() {
        Some("usb") => match &source.usb_type {
            Some(kind) if kind.contains("PD") => format!("{} (USB PD)", c.usb_c_port),
            _ => c.usb_c_port.to_string(),
        },
        _ => c.power_adapter.to_string(),
    }
}

/// "Since unplugged at 14:03" in words, given minutes ago.
pub(crate) fn since_unplugged(
    seconds_on_battery: f64,
    total_watt_hours: f64,
    locale: Locale,
) -> String {
    let span = hours_minutes(seconds_on_battery, locale);
    let energy = watt_hours(total_watt_hours);
    if zh(locale) {
        format!("使用電池 {span}，共耗用 {energy}")
    } else {
        format!("{span} on battery, {energy} used")
    }
}
