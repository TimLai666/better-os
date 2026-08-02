from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one marker, found {count}")
    return text.replace(old, new, 1)


cargo = Path("crates/monitor-gui/Cargo.toml")
text = cargo.read_text()
if '\nzbus = "5"\n' not in text:
    text = replace_once(
        text,
        'sysinfo = "0.37"\n',
        'sysinfo = "0.37"\nzbus = "5"\n',
        "Cargo dependency",
    )
cargo.write_text(text)

settings = Path("crates/monitor-gui/src/settings.rs")
text = settings.read_text()
if "pub window_width: f32" not in text:
    text = replace_once(
        text,
        "    pub locale: Locale,\n    pub last_page: String,\n",
        "    pub locale: Locale,\n    pub last_page: String,\n    pub window_width: f32,\n    pub window_height: f32,\n    pub window_maximized: bool,\n",
        "settings fields",
    )
    text = replace_once(
        text,
        '            locale: Locale::System,\n            last_page: "overview".to_string(),\n',
        '            locale: Locale::System,\n            last_page: "overview".to_string(),\n            window_width: 1360.0,\n            window_height: 860.0,\n            window_maximized: false,\n',
        "settings defaults",
    )
    text = replace_once(
        text,
        "    pub fn clamped_graph_points(&self) -> usize {\n        self.graph_data_points.clamp(30, 600)\n    }\n\n",
        """    pub fn clamped_graph_points(&self) -> usize {
        self.graph_data_points.clamp(30, 600)
    }

    pub fn window_size(&self) -> (f32, f32) {
        (
            self.window_width.clamp(720.0, 8192.0),
            self.window_height.clamp(520.0, 8192.0),
        )
    }

    pub fn remember_window(&mut self, width: f32, height: f32, maximized: bool) {
        if width.is_finite() && height.is_finite() {
            self.window_width = width.clamp(720.0, 8192.0);
            self.window_height = height.clamp(520.0, 8192.0);
        }
        self.window_maximized = maximized;
    }

""",
        "window methods",
    )
    text = replace_once(
        text,
        '            "locale" => self.locale = Locale::parse(value),\n            "last-page" if !value.is_empty() => self.last_page = value.to_string(),\n',
        """            "locale" => self.locale = Locale::parse(value),
            "last-page" if !value.is_empty() => self.last_page = value.to_string(),
            "window-width" => {
                if let Ok(width) = value.parse::<f32>() {
                    self.window_width = width.clamp(720.0, 8192.0);
                }
            }
            "window-height" => {
                if let Ok(height) = value.parse::<f32>() {
                    self.window_height = height.clamp(520.0, 8192.0);
                }
            }
            "window-maximized" => self.window_maximized = Self::bool_value(value),
""",
        "settings parser",
    )
    text = replace_once(
        text,
        '        lines.push(format!("locale={}", self.locale.config_value()));\n        lines.push(format!("last-page={}", self.last_page));\n',
        """        lines.push(format!("locale={}", self.locale.config_value()));
        lines.push(format!("last-page={}", self.last_page));
        let (window_width, window_height) = self.window_size();
        lines.push(format!("window-width={window_width:.0}"));
        lines.push(format!("window-height={window_height:.0}"));
        lines.push(format!("window-maximized={}", self.window_maximized));
""",
        "settings serializer",
    )
    text = replace_once(
        text,
        """    fn last_page_is_serialized_and_loaded() {
        let mut settings = MonitorSettings::default();
        settings.apply("last-page", "network");
        assert_eq!(settings.last_page, "network");
        assert!(settings.to_config().contains("last-page=network"));
    }
""",
        """    fn last_page_is_serialized_and_loaded() {
        let mut settings = MonitorSettings::default();
        settings.apply("last-page", "network");
        assert_eq!(settings.last_page, "network");
        assert!(settings.to_config().contains("last-page=network"));
    }

    #[test]
    fn window_state_is_clamped_and_serialized() {
        let mut settings = MonitorSettings::default();
        settings.apply("window-width", "300");
        settings.apply("window-height", "20000");
        settings.apply("window-maximized", "true");

        assert_eq!(settings.window_size(), (720.0, 8192.0));
        assert!(settings.window_maximized);
        let config = settings.to_config();
        assert!(config.contains("window-width=720"));
        assert!(config.contains("window-height=8192"));
        assert!(config.contains("window-maximized=true"));
    }

    #[test]
    fn invalid_window_samples_do_not_replace_the_last_good_size() {
        let mut settings = MonitorSettings::default();
        settings.remember_window(1200.0, 760.0, false);
        settings.remember_window(f32::NAN, f32::INFINITY, true);

        assert_eq!(settings.window_size(), (1200.0, 760.0));
        assert!(settings.window_maximized);
    }
""",
        "settings tests",
    )
settings.write_text(text)

app = Path("crates/monitor-gui/src/app.rs")
text = app.read_text()
if "fn remember_window_state(&mut self" not in text:
    text = replace_once(
        text,
        "        monitor\n    }\n\n    fn collect_metrics(&mut self, cx: &mut Context<Self>) {\n",
        """        monitor
    }

    fn remember_window_state(&mut self, window: &Window) {
        let bounds = match window.window_bounds() {
            WindowBounds::Windowed(bounds)
            | WindowBounds::Maximized(bounds)
            | WindowBounds::Fullscreen(bounds) => bounds,
        };
        self.settings.remember_window(
            bounds.size.width.as_f32(),
            bounds.size.height.as_f32(),
            window.is_maximized(),
        );
        let _ = self.settings.save();
    }

    fn collect_metrics(&mut self, cx: &mut Context<Self>) {
""",
        "window state method",
    )
    text = replace_once(
        text,
        """        gpui_component::init(cx);
        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1360.0), px(860.0)), cx)),
            ..Default::default()
        };
        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| MonitorWindow::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
""",
        """        gpui_component::init(cx);
        let settings = MonitorSettings::load();
        let (window_width, window_height) = settings.window_size();
        let centered = WindowBounds::centered(size(px(window_width), px(window_height)), cx);
        let window_bounds = if settings.window_maximized {
            let bounds = match centered {
                WindowBounds::Windowed(bounds)
                | WindowBounds::Maximized(bounds)
                | WindowBounds::Fullscreen(bounds) => bounds,
            };
            WindowBounds::Maximized(bounds)
        } else {
            centered
        };
        let window_options = WindowOptions {
            window_bounds: Some(window_bounds),
            ..Default::default()
        };
        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| MonitorWindow::new(window, cx));
                let monitor = view.downgrade();
                window.on_window_should_close(cx, move |window, cx| {
                    if let Some(monitor) = monitor.upgrade() {
                        monitor.update(cx, |monitor, _| monitor.remember_window_state(window));
                    }
                    true
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
""",
        "window launch",
    )
app.write_text(text)

linux = Path("crates/monitor-gui/src/linux.rs")
text = linux.read_text()
if "fn network_manager_dbus_name" not in text:
    text = replace_once(
        text,
        """    path::{Path, PathBuf},
    process::Command,
};

use sysinfo::{Pid, System};
""",
        """    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use sysinfo::{Pid, System};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedObjectPath,
};
""",
        "linux imports",
    )
    old = """fn network_manager_connection_name(interface: &str) -> Option<String> {
    let output = Command::new("nmcli")
        .args([
            "--terse",
            "--get-values",
            "GENERAL.CONNECTION",
            "device",
            "show",
            interface,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty() && value != "--").then_some(value)
}
"""
    new = """const NETWORK_MANAGER_DESTINATION: &str = "org.freedesktop.NetworkManager";
const NETWORK_MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
const NETWORK_MANAGER_INTERFACE: &str = "org.freedesktop.NetworkManager";
const NETWORK_MANAGER_DEVICE_INTERFACE: &str = "org.freedesktop.NetworkManager.Device";
const NETWORK_MANAGER_WIRELESS_INTERFACE: &str =
    "org.freedesktop.NetworkManager.Device.Wireless";
const NETWORK_MANAGER_ACCESS_POINT_INTERFACE: &str =
    "org.freedesktop.NetworkManager.AccessPoint";
const NETWORK_MANAGER_ACTIVE_CONNECTION_INTERFACE: &str =
    "org.freedesktop.NetworkManager.Connection.Active";
const NETWORK_NAME_CACHE_TTL: Duration = Duration::from_secs(5);

type NetworkNameCache = HashMap<String, (Instant, Option<String>)>;

fn usable_network_name(value: impl Into<String>) -> Option<String> {
    let value = value.into();
    let value = value.trim().trim_matches('\0');
    (!value.is_empty() && value != "--").then(|| value.to_string())
}

fn decode_network_manager_ssid(bytes: &[u8]) -> Option<String> {
    usable_network_name(String::from_utf8_lossy(bytes).into_owned())
}

fn network_manager_connection() -> Option<&'static Connection> {
    static CONNECTION: OnceLock<Option<Connection>> = OnceLock::new();
    CONNECTION.get_or_init(|| Connection::system().ok()).as_ref()
}

fn network_manager_dbus_name(interface: &str) -> Option<String> {
    let connection = network_manager_connection()?;
    let manager = Proxy::new(
        connection,
        NETWORK_MANAGER_DESTINATION,
        NETWORK_MANAGER_PATH,
        NETWORK_MANAGER_INTERFACE,
    )
    .ok()?;
    let device_path: OwnedObjectPath = manager.call("GetDeviceByIpIface", &(interface,)).ok()?;
    let device_path = device_path.as_str();

    if let Ok(wireless) = Proxy::new(
        connection,
        NETWORK_MANAGER_DESTINATION,
        device_path,
        NETWORK_MANAGER_WIRELESS_INTERFACE,
    ) && let Ok(access_point_path) =
        wireless.get_property::<OwnedObjectPath>("ActiveAccessPoint")
        && access_point_path.as_str() != "/"
        && let Ok(access_point) = Proxy::new(
            connection,
            NETWORK_MANAGER_DESTINATION,
            access_point_path.as_str(),
            NETWORK_MANAGER_ACCESS_POINT_INTERFACE,
        )
        && let Ok(ssid) = access_point.get_property::<Vec<u8>>("Ssid")
        && let Some(ssid) = decode_network_manager_ssid(&ssid)
    {
        return Some(ssid);
    }

    let device = Proxy::new(
        connection,
        NETWORK_MANAGER_DESTINATION,
        device_path,
        NETWORK_MANAGER_DEVICE_INTERFACE,
    )
    .ok()?;
    let active_connection_path: OwnedObjectPath =
        device.get_property("ActiveConnection").ok()?;
    if active_connection_path.as_str() == "/" {
        return None;
    }
    let active_connection = Proxy::new(
        connection,
        NETWORK_MANAGER_DESTINATION,
        active_connection_path.as_str(),
        NETWORK_MANAGER_ACTIVE_CONNECTION_INTERFACE,
    )
    .ok()?;
    usable_network_name(active_connection.get_property::<String>("Id").ok()?)
}

fn network_manager_nmcli_name(interface: &str) -> Option<String> {
    let output = Command::new("nmcli")
        .args([
            "--terse",
            "--get-values",
            "GENERAL.CONNECTION",
            "device",
            "show",
            interface,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    usable_network_name(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn network_manager_connection_name(interface: &str) -> Option<String> {
    static CACHE: OnceLock<Mutex<NetworkNameCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock()
        && let Some((sampled_at, value)) = cache.get(interface)
        && sampled_at.elapsed() < NETWORK_NAME_CACHE_TTL
    {
        return value.clone();
    }

    let value = network_manager_dbus_name(interface)
        .or_else(|| network_manager_nmcli_name(interface));
    if let Ok(mut cache) = cache.lock() {
        cache.insert(interface.to_string(), (Instant::now(), value.clone()));
    }
    value
}
"""
    text = replace_once(text, old, new, "NetworkManager implementation")
    text += """
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_names_reject_empty_and_placeholder_values() {
        assert_eq!(
            usable_network_name("  Home Wi-Fi  "),
            Some("Home Wi-Fi".to_string())
        );
        assert_eq!(usable_network_name("--"), None);
        assert_eq!(usable_network_name("  "), None);
    }

    #[test]
    fn network_manager_ssid_decoding_is_lossy_but_stable() {
        assert_eq!(
            decode_network_manager_ssid(b"Cafe Wi-Fi"),
            Some("Cafe Wi-Fi".to_string())
        );
        assert_eq!(
            decode_network_manager_ssid(&[0xff, b'A']),
            Some("�A".to_string())
        );
        assert_eq!(decode_network_manager_ssid(b"\0"), None);
    }
}
"""
linux.write_text(text)

checklist = Path("docs/better-monitor-resources-v1.10.2-parity.md")
text = checklist.read_text()
text = text.replace(
    "| Restore window size/maximized state | ⬜ | Not persisted. |",
    "| Restore window size/maximized state | ✅ | Logical window size and maximized state are persisted on close and restored on launch; Wayland remains responsible for final placement. |",
)
text = text.replace(
    "| Wi-Fi SSID | 🟨 | NetworkManager connection names are read through `nmcli` when available; direct D-Bus coverage and real-session validation remain. |",
    "| Wi-Fi SSID | 🧩 | The active access point SSID is read directly from NetworkManager D-Bus, with active connection ID and `nmcli` as fallbacks; real Wi-Fi session validation remains. |",
)
text = text.replace(
    "| Last page/window/maximized | ⬜ | Missing. |",
    "| Last page/window/maximized | ✅ | Last page, logical window size, and maximized state are persisted. Wayland compositor placement is intentionally not fabricated. |",
)
checklist.write_text(text)

Path("docs/better-monitor-window-and-network-state.md").write_text("""# Better Monitor window and network state

## Window state

Better Monitor stores its logical window width, height, and maximized state in
`monitor.conf`. The saved size is clamped before it is used. On Wayland the
compositor remains responsible for placement, so Better Monitor does not store
or claim control over the global window position.

The state is saved from GPUI's close callback using the platform-reported
`WindowBounds` and `is_maximized()` value, then restored through
`WindowOptions.window_bounds` during the next launch.

## NetworkManager metadata

Network names use the system bus first:

1. Resolve the device through `GetDeviceByIpIface`.
2. For Wi-Fi devices, read `ActiveAccessPoint` and its byte-array `Ssid`.
3. Otherwise read the active connection `Id`.
4. Fall back to `nmcli` only when the typed D-Bus path is unavailable.

Results are cached briefly so a fast monitor refresh does not repeatedly query
NetworkManager or spawn helper processes. Unknown values remain unavailable;
Better Monitor does not invent an SSID.
""")
