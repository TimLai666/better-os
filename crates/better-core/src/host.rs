//! What `/etc/os-release` says about the Ubuntu base a host is built on.
//!
//! Three readers need the same answer and must never disagree: `install.sh`
//! decides which published package to download, `manager-daemon` refuses a
//! plan whose target does not match the machine, and the unprivileged client
//! decides which artifact to plan for in the first place. Two of those are
//! Rust and share this module. The third is shell and cannot, so
//! `detect_ubuntu_release` in `install.sh` carries a comment pointing here and
//! the rules below are written to match it line for line — including the
//! `VERSION_CODENAME` fallback, which only plain Ubuntu is trusted for.
//!
//! The file is shell syntax, but nothing here sources or executes it. Fields
//! are read one at a time and unquoted.

/// The Ubuntu releases Better OS publishes packages for.
pub const SUPPORTED_UBUNTU_RELEASES: [&str; 2] = ["22.04", "24.04"];

/// Resolves the Ubuntu base release an os-release file describes.
///
/// Derivatives report their own `VERSION_ID` — Zorin OS 18 says `18`, which
/// names nothing in the release matrix — so the base comes from the codename
/// first. `VERSION_ID` is the fallback, and only for plain Ubuntu: a
/// derivative's version is its own, not the base it was built from.
///
/// `None` means this host is not one Better OS publishes for. It is not a
/// reason to guess: a guess produces packages built for the wrong libc.
pub fn resolve_ubuntu_release(content: &str) -> Option<String> {
    let distribution_id = os_release_field(content, "ID");
    let is_plain_ubuntu = distribution_id.as_deref() == Some("ubuntu");

    let codename = os_release_field(content, "UBUNTU_CODENAME").or_else(|| {
        is_plain_ubuntu
            .then(|| os_release_field(content, "VERSION_CODENAME"))
            .flatten()
    });

    match codename.as_deref() {
        Some("jammy") => Some("22.04".to_string()),
        Some("noble") => Some("24.04".to_string()),
        // A codename outside the matrix is refused rather than falling back to
        // the badge, which would produce a nonsense comparison instead of an
        // honest refusal.
        Some(_) => None,
        None if is_plain_ubuntu => os_release_field(content, "VERSION_ID")
            .filter(|version| SUPPORTED_UBUNTU_RELEASES.contains(&version.as_str())),
        None => None,
    }
}

/// The distribution this host calls itself, as `ID` reports it.
///
/// Manifests declare which distributions they target, so this has to be the
/// real value and not the base: `zorin` is what a Zorin host is, and every
/// first-party manifest names it. A file with no `ID` at all falls back to
/// `ubuntu`, which is the only thing left to say about a host whose codename
/// already resolved to an Ubuntu release.
pub fn resolve_distribution_id(content: &str) -> String {
    os_release_field(content, "ID").unwrap_or_else(|| "ubuntu".to_string())
}

/// What the distribution calls itself, badge version included: `Zorin OS 18`,
/// `Ubuntu 24.04`.
///
/// This is the host's own identity and never the Ubuntu base its packages are
/// built for. Both belong on screen — a Zorin host is Zorin OS 18 *and* built on
/// Ubuntu 24.04 — and showing only `zorin 24.04`, as the window footer did,
/// reads as a Zorin version that does not exist.
///
/// `None` means the file named nothing at all to show.
pub fn describe_distribution(content: &str) -> Option<String> {
    let name = os_release_field(content, "NAME").or_else(|| os_release_field(content, "ID"))?;
    match os_release_field(content, "VERSION_ID") {
        Some(version) => Some(format!("{name} {version}")),
        None => Some(name),
    }
}

/// Pulls one `KEY=` value out of an os-release file, unquoting it.
pub fn os_release_field(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let value = line.trim().strip_prefix(key)?.strip_prefix('=')?;
        let value = value.trim().trim_matches('"').trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

/// A one-line description of what the file said, for a refusal a person has to
/// act on. It names the fields the decision was made from, so a report of an
/// unsupported host carries enough to tell why without asking for the file.
pub fn describe_os_release(content: &str) -> String {
    let pretty = os_release_field(content, "PRETTY_NAME")
        .or_else(|| os_release_field(content, "NAME"))
        .or_else(|| os_release_field(content, "ID"))
        .unwrap_or_else(|| "unknown system".to_string());
    format!(
        "{pretty} (ID={}, VERSION_ID={}, UBUNTU_CODENAME={}, VERSION_CODENAME={})",
        os_release_field(content, "ID").unwrap_or_else(|| "unknown".to_string()),
        os_release_field(content, "VERSION_ID").unwrap_or_else(|| "unknown".to_string()),
        os_release_field(content, "UBUNTU_CODENAME").unwrap_or_else(|| "unset".to_string()),
        os_release_field(content, "VERSION_CODENAME").unwrap_or_else(|| "unset".to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact shape Zorin OS 18.1 ships: `VERSION_ID` names the badge,
    /// `UBUNTU_CODENAME` names the base the packages are built for. Reading the
    /// badge made the daemon refuse every plan on the project's primary target.
    const ZORIN_18: &str = "NAME=\"Zorin OS\"\nID=zorin\nID_LIKE=\"ubuntu debian\"\n\
         PRETTY_NAME=\"Zorin OS 18\"\nVERSION_ID=\"18\"\nUBUNTU_CODENAME=noble\n\
         VERSION_CODENAME=noble\n";
    const ZORIN_17: &str = "NAME=\"Zorin OS\"\nID=zorin\nVERSION_ID=\"17\"\n\
         UBUNTU_CODENAME=jammy\nVERSION_CODENAME=jammy\n";
    const UBUNTU_2404: &str = "NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"24.04\"\n\
         UBUNTU_CODENAME=noble\nVERSION_CODENAME=noble\n";
    const UBUNTU_2204_NO_CODENAME: &str = "NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"22.04\"\n";

    #[test]
    fn zorin_18_resolves_to_its_noble_base_not_its_own_version_id() {
        assert_eq!(resolve_ubuntu_release(ZORIN_18).as_deref(), Some("24.04"));
        assert_eq!(resolve_distribution_id(ZORIN_18), "zorin");
    }

    #[test]
    fn zorin_17_resolves_to_its_jammy_base() {
        assert_eq!(resolve_ubuntu_release(ZORIN_17).as_deref(), Some("22.04"));
    }

    #[test]
    fn plain_ubuntu_resolves_from_its_codename() {
        assert_eq!(
            resolve_ubuntu_release(UBUNTU_2404).as_deref(),
            Some("24.04")
        );
        assert_eq!(resolve_distribution_id(UBUNTU_2404), "ubuntu");
    }

    #[test]
    fn plain_ubuntu_without_a_codename_still_uses_version_id() {
        assert_eq!(
            resolve_ubuntu_release(UBUNTU_2204_NO_CODENAME).as_deref(),
            Some("22.04")
        );
    }

    #[test]
    fn plain_ubuntu_may_use_the_generic_codename_field() {
        // 20.04 and older Ubuntu images carry VERSION_CODENAME without
        // UBUNTU_CODENAME. install.sh accepts that for ID=ubuntu, so this does.
        let content = "ID=ubuntu\nVERSION_ID=\"22.04\"\nVERSION_CODENAME=jammy\n";
        assert_eq!(resolve_ubuntu_release(content).as_deref(), Some("22.04"));
    }

    #[test]
    fn a_derivatives_own_version_is_never_read_as_a_base() {
        // No codename to go on, and a version that names its own badge. The
        // fallback is Ubuntu's alone.
        let content = "ID=zorin\nVERSION_ID=\"18\"\n";
        assert_eq!(resolve_ubuntu_release(content), None);
    }

    #[test]
    fn an_unknown_codename_is_refused_rather_than_falling_back_to_the_badge() {
        let content = "ID=zorin\nVERSION_ID=\"19\"\nUBUNTU_CODENAME=plucky\n";
        assert_eq!(resolve_ubuntu_release(content), None);
    }

    #[test]
    fn an_ubuntu_release_outside_the_matrix_is_refused() {
        let content = "ID=ubuntu\nVERSION_ID=\"20.04\"\nVERSION_CODENAME=focal\n";
        assert_eq!(resolve_ubuntu_release(content), None);
    }

    #[test]
    fn a_system_that_is_not_ubuntu_at_all_is_refused() {
        let content = "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n";
        assert_eq!(resolve_ubuntu_release(content), None);
    }

    #[test]
    fn an_os_release_without_a_version_reports_nothing_rather_than_a_guess() {
        assert_eq!(resolve_ubuntu_release("NAME=\"Zorin OS\"\n"), None);
    }

    #[test]
    fn a_field_is_unquoted_and_not_matched_by_a_longer_key_that_ends_in_it() {
        let content = "ID=ubuntu\nID_LIKE=\"debian\"\nVERSION_ID=\"24.04\"\n";
        assert_eq!(os_release_field(content, "ID").as_deref(), Some("ubuntu"));
        assert_eq!(
            os_release_field(content, "VERSION_ID").as_deref(),
            Some("24.04")
        );
        assert_eq!(os_release_field(content, "MISSING"), None);
    }

    #[test]
    fn a_distribution_describes_itself_by_its_own_name_and_badge() {
        assert_eq!(
            describe_distribution(ZORIN_18).as_deref(),
            Some("Zorin OS 18")
        );
        assert_eq!(
            describe_distribution(UBUNTU_2404).as_deref(),
            Some("Ubuntu 24.04")
        );
        // No NAME to go on: the ID is what is left, and a version is optional.
        assert_eq!(
            describe_distribution("ID=zorin\nVERSION_ID=\"18\"\n").as_deref(),
            Some("zorin 18")
        );
        assert_eq!(
            describe_distribution("NAME=\"Zorin OS\"\n").as_deref(),
            Some("Zorin OS")
        );
        assert_eq!(describe_distribution(""), None);
    }

    #[test]
    fn the_description_names_every_field_the_decision_was_made_from() {
        let described = describe_os_release(ZORIN_18);
        assert!(described.contains("Zorin OS 18"), "{described}");
        assert!(described.contains("ID=zorin"), "{described}");
        assert!(described.contains("VERSION_ID=18"), "{described}");
        assert!(described.contains("UBUNTU_CODENAME=noble"), "{described}");
    }

    #[test]
    fn a_description_of_a_file_that_says_nothing_still_reads_as_a_sentence() {
        let described = describe_os_release("");
        assert!(described.starts_with("unknown system"), "{described}");
        assert!(described.contains("UBUNTU_CODENAME=unset"), "{described}");
    }
}
