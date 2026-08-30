//! Presentation-policy tests that need no display backend.
//!
//! What can be asserted without a window: that every locale answers every
//! question the surface asks it, that an application which does not simply
//! declare the file's type always has something to say about it, and that the
//! window is wide enough for the longest action row in either locale. Launch
//! behavior is covered by the headless smoke run, not here.

use app_catalog_core::{EntryScope, MimeType, NoCanonicalExecutable, SourceKind};
use app_chooser_core::{Compatibility, ExecutableResolution, ExecutableWarning};

use crate::chooser::{
    compatibility_explanation, compatibility_label, executable_message, source_label,
    warning_message,
};
use crate::i18n::{Locale, copy};
use crate::{ActionLayout, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, action_layout};

const LOCALES: [Locale; 2] = [Locale::EnUs, Locale::ZhTw];

fn mime(value: &str) -> MimeType {
    MimeType::parse(value).expect("valid mime type")
}

fn every_compatibility() -> Vec<Compatibility> {
    vec![
        Compatibility::Declares,
        Compatibility::DeclaresRelatedType {
            declared: mime("text/plain"),
            distance: 0,
        },
        Compatibility::DeclaresWildcard {
            pattern: mime("text/*"),
        },
        Compatibility::PreviouslyUsed,
        Compatibility::UserAssociated,
        Compatibility::NotDeclared,
    ]
}

#[test]
fn every_compatibility_reason_has_a_badge_in_every_locale() {
    for locale in LOCALES {
        for compatibility in every_compatibility() {
            assert!(
                !compatibility_label(locale, &compatibility)
                    .trim()
                    .is_empty(),
                "{locale:?} has no badge for {compatibility:?}"
            );
        }
    }
}

#[test]
fn anything_that_does_not_declare_the_type_is_explained_in_every_locale() {
    for locale in LOCALES {
        for compatibility in every_compatibility() {
            let explanation = compatibility_explanation(locale, &compatibility);
            if compatibility.declares_selected_type() {
                assert!(
                    explanation.is_none(),
                    "a declared type needs no explanation"
                );
            } else {
                assert!(
                    explanation.is_some_and(|text| !text.trim().is_empty()),
                    "{locale:?} cannot explain {compatibility:?}"
                );
            }
        }
    }
}

#[test]
fn every_source_kind_and_scope_produces_a_badge_in_every_locale() {
    let kinds = [
        SourceKind::Native,
        SourceKind::Flatpak,
        SourceKind::Snap,
        SourceKind::AppImage,
        SourceKind::Wrapper,
    ];
    for locale in LOCALES {
        for kind in kinds {
            for scope in [EntryScope::User, EntryScope::System] {
                let label = source_label(locale, kind, scope);
                assert!(label.contains('·'), "{locale:?} {kind:?} {scope:?}");
                assert!(label.len() > 3);
            }
        }
    }
}

#[test]
fn every_executable_refusal_says_something_in_every_locale() {
    let refusals = [
        ExecutableWarning::NoSingleExecutable {
            reason: NoCanonicalExecutable::Flatpak,
        },
        ExecutableWarning::DBusActivated,
        ExecutableWarning::ProgramNotFound {
            program: "editor".into(),
        },
        ExecutableWarning::NoExecLine,
        ExecutableWarning::ComplexArguments {
            program: "editor".into(),
            dropped: vec!["--new-window".into()],
        },
        ExecutableWarning::NotFound {
            path: "/usr/bin/nope".into(),
        },
        ExecutableWarning::NotExecutable {
            path: "/etc/hosts".into(),
        },
    ];
    for locale in LOCALES {
        for refusal in &refusals {
            let message =
                executable_message(locale, &ExecutableResolution::Refused(refusal.clone()));
            assert!(!message.trim().is_empty(), "{locale:?} {refusal:?}");
        }
        let resolved =
            executable_message(locale, &ExecutableResolution::Resolved("/usr/bin/x".into()));
        assert!(resolved.contains("/usr/bin/x"));
    }
}

#[test]
fn every_association_warning_is_worded_in_every_locale() {
    use app_chooser_core::AssociationWarning::*;
    for locale in LOCALES {
        for warning in [
            ApplicationDoesNotDeclareType,
            ListedInRemovedAssociations,
            DuplicateDefaultKey,
        ] {
            assert!(!warning_message(locale, &warning).trim().is_empty());
        }
    }
}

#[test]
fn both_locales_fill_in_every_string_the_surface_reads() {
    for locale in LOCALES {
        let c = copy(locale);
        for value in [
            c.open_with_title,
            c.open_with_subtitle,
            c.executable_title,
            c.executable_subtitle,
            c.section_recommended,
            c.section_other,
            c.section_all,
            c.show_all,
            c.hide_all,
            c.search_placeholder,
            c.open_once,
            c.always_use,
            c.cancel,
            c.undo,
            c.loading_title,
            c.loading_detail,
            c.empty_title,
            c.empty_detail,
            c.no_matches_title,
            c.no_matches_detail,
            c.nothing_selected,
            c.badge_default,
            c.launch_failed,
            c.launched,
            c.association_written,
            c.association_failed,
            c.association_unchanged,
            c.association_rolled_back,
            c.executable_use_path,
            c.executable_browse,
            c.executable_browse_hint,
            c.executable_browse_empty,
            c.executable_selected,
        ] {
            assert!(!value.trim().is_empty(), "{locale:?} has an empty string");
        }
    }
}

fn action_labels(locale: Locale) -> [&'static str; 3] {
    let c = copy(locale);
    [c.cancel, c.always_use, c.open_once]
}

#[test]
fn the_minimum_window_holds_the_action_row_in_both_locales() {
    for locale in LOCALES {
        assert_eq!(
            action_layout(MIN_WINDOW_WIDTH, 1.0, &action_labels(locale)),
            ActionLayout::Inline,
            "{locale:?} does not fit the action row at the minimum width"
        );
    }
    // Tall enough for the header, the search field, a section, and the action
    // row without the list collapsing to nothing.
    const { assert!(MIN_WINDOW_HEIGHT >= 480.0) };
}

#[test]
fn a_row_that_no_longer_fits_reports_wrapped_rather_than_overflowing() {
    for locale in LOCALES {
        for scale in [1.25, 1.5] {
            let layout = action_layout(MIN_WINDOW_WIDTH, scale, &action_labels(locale));
            // At a scaled-up interface with no extra window width, the policy
            // must give a definite answer either way; what it must never do is
            // claim a row fits when the arithmetic says otherwise.
            let logical = MIN_WINDOW_WIDTH / scale;
            let fits = layout == ActionLayout::Inline;
            assert_eq!(
                fits,
                action_layout(logical, 1.0, &action_labels(locale)) == ActionLayout::Inline,
                "{locale:?} at {scale}x disagrees with its own logical width"
            );
        }
    }
    assert_eq!(
        action_layout(200.0, 1.0, &action_labels(Locale::EnUs)),
        ActionLayout::Wrapped
    );
}

#[test]
fn the_system_locale_resolves_to_one_of_the_two_shipped_locales() {
    assert!(matches!(
        Locale::System.resolved(),
        Locale::EnUs | Locale::ZhTw
    ));
    assert_eq!(Locale::ZhTw.resolved(), Locale::ZhTw);
    assert!(Locale::ZhTw.entry_locale().is_some());
}
