//! The global keyboard shortcut, described rather than installed.
//!
//! An unprivileged application on GNOME cannot register a system-wide
//! shortcut for itself, and Better OS is not going to grab the keyboard to
//! fake one. What it can do is name exactly which settings carry the shortcut,
//! so the component manifest declares them, the documentation shows them, and
//! Better Defaults (ticket 27) applies them over its own reviewed boundary.
//!
//! Nothing in this module writes a setting or runs `gsettings`. It produces
//! strings.
//!
//! The exact key combination is one of Issue #2's deferred decisions, so
//! [`GnomeCustomKeybinding::binding`] is `None` until that decision is made.
//! A build that shipped a default here would be settling it silently.

/// GNOME's custom-keybinding settings for one shortcut.
///
/// GNOME stores custom shortcuts as a list of relocatable schema paths in
/// `org.gnome.settings-daemon.plugins.media-keys custom-keybindings`, with
/// each path holding a `name`, a `command`, and a `binding`. That is three
/// keys plus one list entry, and all four are named here so nothing has to be
/// rediscovered by whoever wires it up.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GnomeCustomKeybinding {
    /// The relocatable schema each custom shortcut instantiates.
    pub schema: &'static str,
    /// This component's own instance of that schema.
    pub path: &'static str,
    /// The schema holding the list of instantiated paths.
    pub list_schema: &'static str,
    /// The key inside that schema holding the list.
    pub list_key: &'static str,
    /// The `name` the shortcut appears under in Settings.
    pub name: &'static str,
    /// The `command` the shortcut runs. An argument vector's worth of program
    /// name and nothing else: no shell, no arguments interpolated from
    /// anywhere.
    pub command: &'static str,
    /// The `binding` itself. `None` until the exact shortcut is decided.
    pub binding: Option<&'static str>,
}

impl GnomeCustomKeybinding {
    /// Better Launcher's shortcut description.
    pub const fn for_launcher() -> Self {
        Self {
            schema: "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding",
            path: "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/better-launcher/",
            list_schema: "org.gnome.settings-daemon.plugins.media-keys",
            list_key: "custom-keybindings",
            name: "Better Launcher",
            command: "better-launcher",
            binding: None,
        }
    }

    /// Whether the shortcut can be applied yet. It cannot: the key
    /// combination is a deferred decision, and a keybinding with no binding is
    /// not something to write.
    pub fn is_applicable(&self) -> bool {
        self.binding.is_some()
    }

    /// The settings this component touches, in the form the manifest lists
    /// them. Used by the manifest test so the declaration and the code cannot
    /// drift apart.
    pub fn declared_settings(&self) -> Vec<String> {
        vec![
            format!("{}{}", self.path, "name"),
            format!("{}{}", self.path, "command"),
            format!("{}{}", self.path, "binding"),
            format!("{} {}", self.list_schema, self.list_key),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortcut_is_described_but_not_yet_applicable() {
        let keybinding = GnomeCustomKeybinding::for_launcher();
        assert!(
            !keybinding.is_applicable(),
            "the exact shortcut is a deferred decision and must not be hard-coded"
        );
        assert_eq!(keybinding.command, "better-launcher");
    }

    #[test]
    fn every_declared_setting_sits_under_this_component_s_own_path() {
        let keybinding = GnomeCustomKeybinding::for_launcher();
        let settings = keybinding.declared_settings();
        assert_eq!(settings.len(), 4);
        assert!(keybinding.path.ends_with("better-launcher/"));
        for setting in settings.iter().take(3) {
            assert!(setting.starts_with(keybinding.path), "{setting}");
        }
        assert_eq!(
            settings[3],
            "org.gnome.settings-daemon.plugins.media-keys custom-keybindings"
        );
    }

    #[test]
    fn the_command_is_a_program_name_with_no_shell_in_it() {
        let command = GnomeCustomKeybinding::for_launcher().command;
        for character in [' ', ';', '&', '|', '$', '`', '\n'] {
            assert!(
                !command.contains(character),
                "the shortcut command must not be shell-interpretable"
            );
        }
    }
}
