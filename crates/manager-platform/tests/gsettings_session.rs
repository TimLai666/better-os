//! The real `gsettings` controller against a real dconf, in a private session.
//!
//! Ignored by default: it writes `org.gnome.shell enabled-extensions` in
//! whatever session it runs in. Run it only inside a disposable session whose
//! HOME and bus are scratch, with `BETTER_OS_PRIVATE_GSETTINGS_SESSION=1`:
//!
//! ```text
//! HOME=$(mktemp -d) XDG_CONFIG_HOME=$HOME/.config XDG_RUNTIME_DIR=... \
//!   dbus-run-session -- env BETTER_OS_PRIVATE_GSETTINGS_SESSION=1 \
//!   cargo test -p manager-platform --test gsettings_session -- --ignored
//! ```

use manager_platform::shell_extensions::{
    ENABLED_KEY, GsettingsShellExtensions, ShellExtensionController, ShellSettings,
};

#[test]
#[ignore = "writes GNOME Shell settings; run only in a private session"]
fn enable_and_disable_through_real_dconf() {
    assert_eq!(
        std::env::var("BETTER_OS_PRIVATE_GSETTINGS_SESSION").as_deref(),
        Ok("1"),
        "refusing to touch a session that is not declared private"
    );
    let controller = GsettingsShellExtensions::from_environment();
    let ours = vec!["touchpad-adapter@betteros.org".to_string()];
    let before = controller.settings().get(ENABLED_KEY).unwrap();

    controller.enable(&ours).unwrap();
    let on = controller.settings().get(ENABLED_KEY).unwrap();
    assert!(on.ends_with(&ours), "{on:?}");
    assert!(before.iter().all(|entry| on.contains(entry)));
    assert_eq!(controller.enabled(&ours).unwrap(), ours);

    controller.disable(&ours).unwrap();
    let off = controller.settings().get(ENABLED_KEY).unwrap();
    assert_eq!(
        off,
        before
            .into_iter()
            .filter(|e| !ours.contains(e))
            .collect::<Vec<_>>()
    );
}
