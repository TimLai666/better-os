//! The contract between the Better OS GNOME Shell adapter extension
//! (`adapters/gnome-shell-adapter/`) and the window attention service, as
//! ADR 0018 fixes it.
//!
//! The extension owns [`ADAPTER_BUS_NAME`] and serves [`ADAPTER_INTERFACE`]
//! at [`ADAPTER_OBJECT_PATH`]. It reports attention requests and performs
//! four shell actions, only while [`SERVICE_BUS_NAME`] has an owner, and only
//! for that owner.
//! The interface file beside the extension declares exactly these members;
//! `window-attention-service`'s `tests/extension.rs` holds the two together.

/// The shared extension's uuid, which is also its directory name.
pub const EXTENSION_UUID: &str = "shell-adapter@betteros.org";

/// The extension's bus name.
pub const ADAPTER_BUS_NAME: &str = "org.betteros.ShellAdapter1";
/// Where the window attention relay is served.
pub const ADAPTER_OBJECT_PATH: &str = "/org/betteros/ShellAdapter1/WindowAttention";
/// The window attention relay's interface.
pub const ADAPTER_INTERFACE: &str = "org.betteros.ShellAdapter1.WindowAttention";
/// The interface file that ships beside the extension.
pub const ADAPTER_INTERFACE_FILE: &str = "org.betteros.ShellAdapter1.WindowAttention.xml";

/// `WindowAttention(t window, s app_id, s app_name, u kind, b focused,
/// b skip_taskbar)`.
pub const WINDOW_ATTENTION_SIGNAL: &str = "WindowAttention";
/// The signal's body signature.
pub const WINDOW_ATTENTION_SIGNATURE: &str = "tssubb";
/// `WindowAttentionEnded(t window, u reason)`: a reported window stopped
/// asking (0 focused, 1 closed, 2 withdrawn).
pub const WINDOW_ATTENTION_ENDED_SIGNAL: &str = "WindowAttentionEnded";
pub const WINDOW_ATTENTION_ENDED_SIGNATURE: &str = "tu";

/// `SuppressBuiltInAttention(b suppress)`: take GNOME's own handler off the
/// two display signals, or put it back.
pub const SUPPRESS_METHOD: &str = "SuppressBuiltInAttention";
/// `ShowAttentionBanner(t window)`: run GNOME's own handler for the window.
pub const SHOW_BANNER_METHOD: &str = "ShowAttentionBanner";
/// `ActivateWindow(t window)`: bring the window forward.
pub const ACTIVATE_METHOD: &str = "ActivateWindow";
/// `ClearAttention(t window)`: take the request off the window.
pub const CLEAR_METHOD: &str = "ClearAttention";
/// `Capabilities() -> s`: a JSON report, reading and changing nothing.
pub const CAPABILITIES_METHOD: &str = "Capabilities";

/// Every method the relay serves.
pub const ADAPTER_METHODS: [&str; 5] = [
    SUPPRESS_METHOD,
    SHOW_BANNER_METHOD,
    ACTIVATE_METHOD,
    CLEAR_METHOD,
    CAPABILITIES_METHOD,
];
/// Every signal the relay emits.
pub const ADAPTER_SIGNALS: [&str; 2] = [WINDOW_ATTENTION_SIGNAL, WINDOW_ATTENTION_ENDED_SIGNAL];

/// The service's bus name. The extension relays only while it has an owner.
pub const SERVICE_BUS_NAME: &str = "org.betteros.WindowAttention1";
pub const SERVICE_OBJECT_PATH: &str = "/org/betteros/WindowAttention1";
pub const SERVICE_INTERFACE: &str = "org.betteros.WindowAttention1";
/// `ChoicesChanged()`: Better Manager saved the choices. The service takes back
/// every taskbar mark the new choices no longer ask for.
pub const CHOICES_CHANGED_METHOD: &str = "ChoicesChanged";

/// The protocol version both halves report.
pub const PROTOCOL_VERSION: u32 = 1;
