//! The rule for whether an installed package is sound, shared by everything
//! that has to answer that question.
//!
//! dpkg's own file database says what a package put on disk, and the machine
//! says what is there now. A package is sound when dpkg reports it installed and
//! every file dpkg lists for it that has to exist does. Three kinds of listed
//! path need not exist, each because its absence is not evidence of a broken
//! package:
//!
//! - directories, which are shared between packages and say nothing about this
//!   one;
//! - conffiles, because a machine's owner is allowed to delete one and dpkg
//!   does not consider the package broken when they have;
//! - paths this machine's dpkg was configured never to unpack, which it lists
//!   all the same. [`PathFilter`] reads that configuration.
//!
//! Nothing here executes a rule or trusts a manifest. It reads dpkg's own
//! configuration and file list, which are the only things that can say a file's
//! absence was deliberate.

use std::path::{Path, PathBuf};

/// What the filesystem says about one path dpkg listed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathState {
    /// Nothing is there.
    Missing,
    /// A directory, which several packages may share and which is not evidence
    /// about this one.
    Directory,
    /// A file, a symlink, or anything else that exists at that path.
    Present,
}

pub trait HealthProbe: Send + Sync {
    /// What is at this path, without following symlinks: a link that exists is
    /// present whether or not its target does, because the link is the file the
    /// package installed.
    fn inspect(&self, path: &Path) -> PathState;
}

pub struct SystemHealthProbe;

impl HealthProbe for SystemHealthProbe {
    fn inspect(&self, path: &Path) -> PathState {
        match std::fs::symlink_metadata(path) {
            Err(_) => PathState::Missing,
            Ok(metadata) if metadata.is_dir() => PathState::Directory,
            Ok(_) => PathState::Present,
        }
    }
}

/// A probe that answers from a fixed list, for tests. Paths in the list are
/// present, paths in `directories` are directories, and anything else is
/// missing.
pub struct FakeHealthProbe {
    pub present: Vec<PathBuf>,
    pub directories: Vec<PathBuf>,
}

impl FakeHealthProbe {
    pub fn with_files(paths: Vec<PathBuf>) -> Self {
        Self {
            present: paths,
            directories: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        Self {
            present: Vec::new(),
            directories: Vec::new(),
        }
    }

    pub fn with_directories(mut self, paths: Vec<PathBuf>) -> Self {
        self.directories = paths;
        self
    }
}

impl HealthProbe for FakeHealthProbe {
    fn inspect(&self, path: &Path) -> PathState {
        if self.directories.iter().any(|known| known == path) {
            PathState::Directory
        } else if self.present.iter().any(|known| known == path) {
            PathState::Present
        } else {
            PathState::Missing
        }
    }
}

/// One `path-exclude` or `path-include` rule, in the order dpkg would apply it.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Rule {
    excluded: bool,
    pattern: String,
}

/// dpkg's configured path filters, newest rule last.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PathFilter {
    rules: Vec<Rule>,
}

impl PathFilter {
    /// The filters in effect for this machine.
    ///
    /// `config_root` is normally `/etc/dpkg`. Fragments are read in name order
    /// and then `dpkg.cfg`, and only fragment names dpkg itself accepts —
    /// letters, digits, `_` and `-` — are read, because dpkg ignores the rest
    /// and a filter this reads but dpkg does not would make an absent file look
    /// deliberate when it is not.
    pub fn from_config_dir(config_root: &Path) -> Self {
        let mut fragments: Vec<PathBuf> = std::fs::read_dir(config_root.join("dpkg.cfg.d"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| {
                                !name.is_empty()
                                    && name.chars().all(|character| {
                                        character.is_ascii_alphanumeric()
                                            || character == '_'
                                            || character == '-'
                                    })
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        fragments.sort();
        fragments.push(config_root.join("dpkg.cfg"));

        let mut filter = Self::default();
        for fragment in fragments {
            if let Ok(content) = std::fs::read_to_string(&fragment) {
                filter.extend_from_config(&content);
            }
        }
        filter
    }

    /// Adds the rules a dpkg configuration file declares, in file order.
    pub fn extend_from_config(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            // dpkg accepts a leading `--` on a configuration line as well as on
            // the command line.
            let line = line.strip_prefix("--").unwrap_or(line);
            self.push_option(line);
        }
    }

    /// Adds one rule written the way a command-line option is:
    /// `path-exclude=/usr/share/doc/*`.
    pub fn push_option(&mut self, option: &str) {
        let option = option.trim().trim_start_matches("--");
        let Some((key, pattern)) = option.split_once('=') else {
            return;
        };
        let pattern = pattern.trim().trim_matches('"').trim();
        if pattern.is_empty() {
            return;
        }
        match key.trim() {
            "path-exclude" => self.rules.push(Rule {
                excluded: true,
                pattern: pattern.to_string(),
            }),
            "path-include" => self.rules.push(Rule {
                excluded: false,
                pattern: pattern.to_string(),
            }),
            _ => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Whether dpkg was configured not to install this path.
    ///
    /// The last rule that matches decides, which is what dpkg documents.
    pub fn excludes(&self, path: &Path) -> bool {
        let path = path.to_string_lossy();
        self.rules
            .iter()
            .rev()
            .find(|rule| glob_matches(&rule.pattern, &path))
            .is_some_and(|rule| rule.excluded)
    }
}

/// dpkg's own glob rules: `*` matches any run of characters including `/`,
/// `?` matches one character, and `[...]` is a character class. The whole path
/// must match, which is what dpkg's filters compare against.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut pattern_index, mut text_index) = (0usize, 0usize);
    // Where to resume if a `*` turns out to have consumed too little.
    let mut star: Option<(usize, usize)> = None;

    while text_index < text.len() {
        if pattern.get(pattern_index) == Some(&'*') {
            star = Some((pattern_index, text_index));
            pattern_index += 1;
            continue;
        }

        // Where the pattern continues if this character matches. `None` means
        // it did not.
        let consumed = match pattern.get(pattern_index) {
            Some('[') => match class_matches(&pattern, pattern_index, text[text_index]) {
                Some((true, next)) => Some(next),
                Some((false, _)) => None,
                // An unterminated class is a literal bracket, which is what a
                // shell does with one too.
                None => (text[text_index] == '[').then_some(pattern_index + 1),
            },
            Some('?') => Some(pattern_index + 1),
            Some(character) if *character == text[text_index] => Some(pattern_index + 1),
            _ => None,
        };

        match consumed {
            Some(next) => {
                pattern_index = next;
                text_index += 1;
            }
            None => {
                // Give the last `*` one more character and try again.
                let Some((star_pattern, star_text)) = star else {
                    return false;
                };
                pattern_index = star_pattern + 1;
                text_index = star_text + 1;
                star = Some((star_pattern, text_index));
            }
        }
    }

    pattern[pattern_index..].iter().all(|item| *item == '*')
}

/// Reads one `[...]` class at `open`. Returns whether it matched and where the
/// pattern continues, or `None` when the class is unterminated — in which case
/// `[` is not a class at all and the caller falls back to a literal comparison.
fn class_matches(pattern: &[char], open: usize, candidate: char) -> Option<(bool, usize)> {
    let mut index = open + 1;
    let negated = matches!(pattern.get(index), Some('!') | Some('^'));
    if negated {
        index += 1;
    }
    let mut matched = false;
    let mut first = true;
    while index < pattern.len() {
        let character = pattern[index];
        if character == ']' && !first {
            return Some((matched != negated, index + 1));
        }
        first = false;
        // A range, unless the `-` is the last character before the `]`.
        if pattern.get(index + 1) == Some(&'-')
            && pattern.get(index + 2).is_some_and(|end| *end != ']')
        {
            let end = pattern[index + 2];
            if (character..=end).contains(&candidate) {
                matched = true;
            }
            index += 3;
            continue;
        }
        if character == candidate {
            matched = true;
        }
        index += 1;
    }
    None
}

/// What dpkg's own file database says a package put on disk.
///
/// `paths` is everything `dpkg-query -L` listed, directories included, in the
/// order dpkg reported. `conffiles` is the subset dpkg treats as
/// administrator-owned configuration, which a machine's owner is allowed to
/// delete without the package being broken. `excluded` is the subset this
/// machine's dpkg was configured never to unpack, which is listed all the same.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstalledFiles {
    pub paths: Vec<PathBuf>,
    pub conffiles: Vec<PathBuf>,
    pub excluded: Vec<PathBuf>,
}

impl InstalledFiles {
    /// Whether this path has to be on disk for the package to be sound.
    ///
    /// A conffile need not: dpkg lets a machine's owner delete one. A
    /// path-excluded file need not: dpkg was told not to install it.
    pub fn is_required(&self, path: &Path) -> bool {
        !self.conffiles.iter().any(|known| known == path)
            && !self.excluded.iter().any(|known| known == path)
    }
}

/// The value out of one `apt-config dump` line, which is written
/// `DPkg::Options:: "--path-exclude=/usr/share/doc/*";`.
pub fn parse_apt_config_value(line: &str) -> Option<&str> {
    let (_, value) = line.trim().split_once(' ')?;
    let value = value.trim().trim_end_matches(';').trim_matches('"').trim();
    (!value.is_empty()).then_some(value)
}

/// Keeps the absolute paths out of a `dpkg-query -L` listing.
///
/// dpkg prints one path per line and, for a package involved in a diversion,
/// explanatory lines that are not paths. Only lines that start at the root are
/// files this package is answerable for.
pub fn parse_file_list(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .map(str::trim_end)
        .filter(|line| line.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// Reads the paths out of dpkg's `${Conffiles}` field, which is one
/// ` /path hash [obsolete]` line per configuration file.
pub fn parse_conffiles(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|path| path.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

/// What the shared rule concluded about one package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HealthVerdict {
    Healthy,
    Failed(String),
    Undetermined(String),
}

/// Installed and sound: dpkg reports `installed`, and every path in `files`
/// that `is_required` exists according to `probe` (directories count as
/// present). `files` is only called when the package is installed.
pub fn check_installed(
    component: &str,
    installed: Result<Option<String>, String>,
    files: impl FnOnce() -> Result<InstalledFiles, String>,
    probe: &dyn HealthProbe,
) -> HealthVerdict {
    let installed = match installed {
        Ok(installed) => installed,
        Err(error) => {
            return HealthVerdict::Undetermined(format!("dpkg query failed: {error}"));
        }
    };
    let Some(version) = installed else {
        return HealthVerdict::Failed(format!("{component} is not installed"));
    };
    let files = match files() {
        Ok(files) => files,
        Err(error) => {
            return HealthVerdict::Undetermined(format!(
                "the dpkg file list for {component} could not be read: {error}"
            ));
        }
    };
    for path in &files.paths {
        if !files.is_required(path) {
            continue;
        }
        match probe.inspect(path) {
            PathState::Present | PathState::Directory => {}
            PathState::Missing => {
                return HealthVerdict::Failed(format!(
                    "{component} reports version {version} but {} is missing, \
                     though dpkg lists it as installed",
                    path.display()
                ));
            }
        }
    }
    HealthVerdict::Healthy
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim from the `ubuntu:24.04` image's
    /// `/etc/dpkg/dpkg.cfg.d/excludes`, which is why the container end-to-end
    /// check sees a package whose documentation was never unpacked.
    const DOCKER_EXCLUDES: &str = "# Drop all man pages\n\
         path-exclude=/usr/share/man/*\n\
         \n\
         # Drop all translations\n\
         path-exclude=/usr/share/locale/*/LC_MESSAGES/*.mo\n\
         \n\
         # Drop all documentation ...\n\
         path-exclude=/usr/share/doc/*\n\
         # ... except copyright files ...\n\
         path-include=/usr/share/doc/*/copyright\n";

    fn docker() -> PathFilter {
        let mut filter = PathFilter::default();
        filter.extend_from_config(DOCKER_EXCLUDES);
        filter
    }

    #[test]
    fn a_minimized_image_excludes_documentation_but_keeps_the_copyright_file() {
        let filter = docker();
        assert!(filter.excludes(Path::new(
            "/usr/share/doc/better-monitor/THIRD-PARTY-LICENSES.md"
        )));
        // The later include wins, which is the rule dpkg documents.
        assert!(!filter.excludes(Path::new("/usr/share/doc/better-monitor/copyright")));
        assert!(filter.excludes(Path::new("/usr/share/man/man1/better-monitor.1.gz")));
        assert!(filter.excludes(Path::new("/usr/share/locale/de/LC_MESSAGES/x.mo")));
    }

    #[test]
    fn nothing_a_package_needs_is_excluded_by_those_rules() {
        let filter = docker();
        for path in [
            "/usr/bin/better-awake-service",
            "/usr/bin/awake-tray",
            "/usr/libexec/better-manager-daemon",
            "/usr/share/applications/better-awake.desktop",
            "/usr/share/icons/hicolor/scalable/apps/better-awake.svg",
            "/usr/lib/systemd/user/better-awake.service",
        ] {
            assert!(!filter.excludes(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn a_machine_with_no_filters_excludes_nothing() {
        let filter = PathFilter::default();
        assert!(filter.is_empty());
        assert!(!filter.excludes(Path::new("/usr/share/doc/better-monitor/copyright")));
    }

    #[test]
    fn comments_and_unrelated_options_are_not_rules() {
        let mut filter = PathFilter::default();
        filter.extend_from_config(
            "# path-exclude=/usr/bin/*\nforce-confold\nno-pager\npath-exclude=\n--path-exclude=/opt/*\n",
        );
        assert!(!filter.excludes(Path::new("/usr/bin/better-awake-service")));
        assert!(filter.excludes(Path::new("/opt/anything")));
    }

    #[test]
    fn a_filter_passed_as_an_apt_option_counts_too() {
        let mut filter = PathFilter::default();
        filter.push_option("--path-exclude=/usr/share/doc/*");
        assert!(filter.excludes(Path::new("/usr/share/doc/better-files/copyright")));
    }

    #[test]
    fn the_glob_rules_are_dpkgs_own() {
        // `*` crosses directory separators, unlike a shell's own globbing.
        assert!(glob_matches(
            "/usr/*/copyright",
            "/usr/share/doc/x/copyright"
        ));
        assert!(glob_matches("/usr/share/doc/*", "/usr/share/doc/x/y/z"));
        assert!(!glob_matches("/usr/share/doc/*", "/usr/share/docs"));
        assert!(glob_matches("/usr/bin/?", "/usr/bin/a"));
        assert!(!glob_matches("/usr/bin/?", "/usr/bin/ab"));
        assert!(glob_matches(
            "/usr/share/man/man[1-9]/*",
            "/usr/share/man/man3/x"
        ));
        assert!(!glob_matches(
            "/usr/share/man/man[1-9]/*",
            "/usr/share/man/mana/x"
        ));
        assert!(glob_matches("/x[!a]y", "/xby"));
        assert!(!glob_matches("/x[!a]y", "/xay"));
        // An unterminated class is a literal bracket, not a syntax error.
        assert!(glob_matches("/usr/[bin", "/usr/[bin"));
        assert!(glob_matches("*", "/anything/at/all"));
        assert!(!glob_matches("/usr/bin/x", "/usr/bin/xy"));
    }

    #[test]
    fn a_config_directory_is_read_in_order_and_ignores_names_dpkg_ignores() {
        let root = std::env::temp_dir().join(format!(
            "better-os-dpkg-config-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let fragments = root.join("dpkg.cfg.d");
        std::fs::create_dir_all(&fragments).unwrap();
        std::fs::write(fragments.join("excludes"), DOCKER_EXCLUDES).unwrap();
        // dpkg reads no fragment whose name has a dot in it, so neither does
        // this: a rule dpkg ignores must not make a missing file look
        // deliberate.
        std::fs::write(
            fragments.join("local.disabled"),
            "path-exclude=/usr/bin/*\n",
        )
        .unwrap();
        std::fs::write(
            root.join("dpkg.cfg"),
            "path-include=/usr/share/doc/*/NOTICE\n",
        )
        .unwrap();

        let filter = PathFilter::from_config_dir(&root);
        assert!(filter.excludes(Path::new("/usr/share/doc/x/README")));
        assert!(!filter.excludes(Path::new("/usr/share/doc/x/NOTICE")));
        assert!(!filter.excludes(Path::new("/usr/bin/better-awake-service")));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_machine_without_a_dpkg_config_directory_is_not_an_error() {
        let filter = PathFilter::from_config_dir(Path::new("/nonexistent/better-os/etc/dpkg"));
        assert!(filter.is_empty());
    }

    #[test]
    fn a_file_listing_keeps_paths_and_drops_dpkgs_prose() {
        // The second line is what dpkg prints for a diverted file. It is not a
        // path this package has to have on disk.
        let listed = parse_file_list(
            "/.\n\
             package diverts others to: /usr/bin/better-awake-service.distrib\n\
             /usr/bin\n\
             /usr/bin/better-awake-service\n\
             /usr/bin/awake-tray\n",
        );
        assert_eq!(
            listed,
            vec![
                PathBuf::from("/."),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/usr/bin/better-awake-service"),
                PathBuf::from("/usr/bin/awake-tray"),
            ]
        );
    }

    #[test]
    fn conffile_paths_are_read_without_their_checksums() {
        let conffiles = parse_conffiles(
            " /etc/better-os/awake.conf 3d5e0c2f9a1b4c6d7e8f90a1b2c3d4e5\n\
             /etc/better-os/old.conf 0123456789abcdef0123456789abcdef obsolete\n",
        );
        assert_eq!(
            conffiles,
            vec![
                PathBuf::from("/etc/better-os/awake.conf"),
                PathBuf::from("/etc/better-os/old.conf"),
            ]
        );
        assert!(parse_conffiles("").is_empty());
    }

    #[test]
    fn an_apt_config_line_yields_the_option_apt_would_pass_to_dpkg() {
        // The shape `apt-config dump` prints, confirmed against apt on the host.
        assert_eq!(
            parse_apt_config_value("DPkg::Options:: \"--path-exclude=/usr/share/doc/*\";"),
            Some("--path-exclude=/usr/share/doc/*")
        );
        assert_eq!(
            parse_apt_config_value("DPkg::Options:: \"--force-confold\";"),
            Some("--force-confold")
        );
        assert_eq!(parse_apt_config_value(""), None);
        assert_eq!(parse_apt_config_value("DPkg::Options \"\";"), None);
    }

    #[test]
    fn the_probe_reads_the_link_and_not_what_it_points_at() {
        let root = std::env::temp_dir().join(format!(
            "better-os-health-probe-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("file");
        std::fs::write(&file, b"x").unwrap();
        let dangling = root.join("dangling");
        std::os::unix::fs::symlink(root.join("no-such-target"), &dangling).unwrap();

        let probe = SystemHealthProbe;
        assert_eq!(probe.inspect(&root), PathState::Directory);
        assert_eq!(probe.inspect(&file), PathState::Present);
        assert_eq!(probe.inspect(&dangling), PathState::Present);
        assert_eq!(probe.inspect(&root.join("absent")), PathState::Missing);

        std::fs::remove_dir_all(&root).unwrap();
    }
}
