use better_core::ComponentIcon;
use better_ui::icon_set::Picture;
use better_ui::{StatusPill, StatusTone};
use manager_core::{ComponentStatus, DesiredOperation, RestartRequirement};

use crate::{
    app::ManagerApp,
    i18n::{Locale, copy},
    model::{ComponentInfo, ComponentKind},
};

impl ManagerApp {
    /// How one component's state reads. An associated function taking a
    /// locale rather than a method, so a test can enumerate every state
    /// without opening a window.
    pub(crate) fn status_pill(
        locale: Locale,
        status: ComponentStatus,
        pending: bool,
    ) -> StatusPill {
        let c = copy(locale);
        if pending {
            return StatusPill::new(c.ready_to_install, StatusTone::Info);
        }
        let (label, tone) = match status {
            ComponentStatus::Available => (c.available, StatusTone::Info),
            ComponentStatus::Downloading => (c.downloading, StatusTone::Info),
            ComponentStatus::ReadyToInstall => (c.ready_to_install, StatusTone::Info),
            ComponentStatus::Installing => (c.installing_files, StatusTone::Info),
            ComponentStatus::Verifying => (c.checking_works, StatusTone::Info),
            ComponentStatus::Healthy => (c.healthy, StatusTone::Success),
            ComponentStatus::UpdateAvailable => (c.update_available, StatusTone::Warning),
            ComponentStatus::Disabled => (c.disabled, StatusTone::Neutral),
            ComponentStatus::Incompatible => (c.incompatible, StatusTone::Danger),
            ComponentStatus::Degraded => (c.degraded, StatusTone::Warning),
            ComponentStatus::Failed => (c.failed, StatusTone::Danger),
            ComponentStatus::RestoreAvailable => (c.restore_available_status, StatusTone::Danger),
        };
        StatusPill::new(label, tone)
    }

    pub(crate) fn kind_pill(locale: Locale, kind: ComponentKind) -> StatusPill {
        let c = copy(locale);
        let (label, tone) = match kind {
            ComponentKind::Replacement => (c.replacement, StatusTone::Info),
            ComponentKind::Enhancement => (c.enhancement, StatusTone::Success),
            ComponentKind::Diagnostic => (c.diagnostic, StatusTone::Neutral),
        };
        StatusPill::new(label, tone)
    }

    #[cfg(test)]
    pub(crate) fn health_pill(locale: Locale, health: manager_core::HealthState) -> StatusPill {
        let c = copy(locale);
        let (label, tone) = match health {
            manager_core::HealthState::Healthy => (c.healthy, StatusTone::Success),
            manager_core::HealthState::Degraded => (c.degraded, StatusTone::Warning),
            manager_core::HealthState::Failed => (c.failed, StatusTone::Danger),
        };
        StatusPill::new(label, tone)
    }

    /// Maps the icon a manifest declares onto a shipped glyph. The manifest
    /// chooses from a closed set, so this never guesses from a component ID.
    pub(crate) fn component_icon(&self, icon: ComponentIcon) -> Picture {
        manifest_glyph(icon)
    }

    /// A component's glyph in the Apps list: the shipped one for a
    /// first-party component this build knows (audit M13: Better Awake, Better
    /// Git, and Better Storage all declare the generic icon, because the
    /// manifest's icon set is closed and a released manager refuses a value
    /// it does not know), the manifest's otherwise.
    pub(crate) fn component_glyph(&self, component: &ComponentInfo) -> Picture {
        first_party_glyph(component.core_id.as_str()).unwrap_or(manifest_glyph(component.icon))
    }
}

/// The glyph this build ships for a first-party component whose manifest can
/// only say "generic". Like the shipped names and descriptions, it is keyed by
/// the component's id, which is this project's own.
pub(crate) fn first_party_glyph(id: &str) -> Option<Picture> {
    match id {
        "better-awake" => Some(Picture::KeepAwake),
        "better-git" => Some(Picture::Branch),
        "better-storage" => Some(Picture::Drive),
        _ => None,
    }
}

pub(crate) fn manifest_glyph(icon: ComponentIcon) -> Picture {
    {
        match icon {
            ComponentIcon::Manager => Picture::Settings,
            ComponentIcon::Monitor => Picture::Chart,
            ComponentIcon::Files => Picture::Folder,
            ComponentIcon::Launcher => Picture::Apps,
            ComponentIcon::Touchpad => Picture::Touchpad,
            ComponentIcon::Generic => Picture::Package,
        }
    }
}

impl ManagerApp {
    /// What happens after `operation`, for the "Afterwards" row; `None`
    /// hides the row. See [`restart_effect`].
    pub(crate) fn restart_requirement_label(
        &self,
        operation: DesiredOperation,
        requirement: RestartRequirement,
    ) -> Option<&'static str> {
        restart_effect(self.locale, operation, requirement)
    }
}

/// What happens after `operation`, given the manifest's restart requirement.
///
/// `manager-core` attaches the manifest's requirement to every operation that
/// changes a component, but ticket 105's automatic reopening only happens
/// when a running app is *updated*: the window notices its own file was
/// replaced and starts the new program. So only an update says it reopens by
/// itself, and only "if it is open"; installing, enabling, or restoring takes
/// effect the next time the app opens; disabling or removing an app that
/// only needed restarting has nothing left to say. Logging out and
/// restarting the computer are the person's to do, whatever the operation.
pub(crate) fn restart_effect(
    locale: Locale,
    operation: DesiredOperation,
    requirement: RestartRequirement,
) -> Option<&'static str> {
    let c = copy(locale);
    match requirement {
        RestartRequirement::NotDeclared => Some(c.not_declared),
        RestartRequirement::NotRequired => Some(c.restart_not_required),
        RestartRequirement::LogOut => Some(c.restart_log_out),
        RestartRequirement::Reboot => Some(c.restart_reboot),
        RestartRequirement::RestartApplication => match operation {
            DesiredOperation::Update => Some(c.restart_application),
            DesiredOperation::Install | DesiredOperation::Enable | DesiredOperation::Restore => {
                Some(c.restart_next_open)
            }
            DesiredOperation::Disable
            | DesiredOperation::Remove
            | DesiredOperation::Verify
            | DesiredOperation::AddCompanions => None,
        },
    }
}
