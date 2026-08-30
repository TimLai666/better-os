//! What must not leave the machine.
//!
//! The export exists so somebody else can read it, which is exactly why it is
//! the most dangerous thing Better Monitor does. This module is the boundary:
//! every string that goes into a package passes through [`Redactor::apply`],
//! and every replacement it makes is counted so the package can tell its
//! reader what was taken out.
//!
//! Two kinds of rule run here, and both are needed.
//!
//! *Known values* come from the inventory: this machine's hostname, this
//! user's name, this user's home directory, the MAC and IP addresses of its
//! links. They are collected once, classified where they are collected, and
//! replaced literally.
//!
//! *Shaped values* are found by scanning: anything that looks like an address
//! or a credential, whether or not the inventory ever saw it. A token pasted
//! into an incident note was never in the inventory and never will be, so
//! recognising its shape is the only way to catch it.
//!
//! Command lines are handled by a third rule that does not scan at all: every
//! argument after the program name is dropped outright. Arguments are where
//! credentials, file paths, and search terms actually live, and no scanner is
//! good enough to be trusted with them.

use std::collections::BTreeMap;

use monitor_store::{Inventory, Sensitivity};
use serde::{Deserialize, Serialize};

/// The redaction policy version. It goes in the package so a reader can tell
/// which rules produced the file in front of them.
pub const REDACTION_POLICY_VERSION: u32 = 1;

/// The shortest run of credential-shaped characters that is treated as a
/// secret.
///
/// Twenty is above every metric identifier, process name, systemd unit, and
/// device name this project produces, and below every API token, session key,
/// and base64 blob worth worrying about. It is a threshold, not a proof, which
/// is why the command-line rule does not depend on it.
pub const MINIMUM_TOKEN_LENGTH: usize = 20;

/// The rules, as stable keys. The report names them; a locale table can turn
/// them into sentences.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Everything after the program name in a command line.
    CommandArguments,
    /// The path of this user's home directory, wherever it appears.
    HomePath,
    /// This user's login name.
    Username,
    /// This machine's hostname.
    Hostname,
    /// A hardware address.
    MacAddress,
    /// An IPv4 address.
    IpV4Address,
    /// An IPv6 address.
    IpV6Address,
    /// Any other machine identifier the inventory classified as one: a disk
    /// serial, a machine id.
    Identifier,
    /// A run of characters shaped like a credential.
    Token,
}

impl Rule {
    pub fn key(self) -> &'static str {
        match self {
            Rule::CommandArguments => "command_arguments",
            Rule::HomePath => "home_path",
            Rule::Username => "username",
            Rule::Hostname => "hostname",
            Rule::MacAddress => "mac_address",
            Rule::IpV4Address => "ipv4_address",
            Rule::IpV6Address => "ipv6_address",
            Rule::Identifier => "identifier",
            Rule::Token => "token",
        }
    }

    /// What replaces a match. A placeholder rather than an empty string, so a
    /// reader can see that something was there.
    pub fn placeholder(self) -> &'static str {
        match self {
            Rule::CommandArguments => "[arguments withheld]",
            Rule::HomePath => "<home>",
            Rule::Username => "<user>",
            Rule::Hostname => "<host>",
            Rule::MacAddress => "<mac>",
            Rule::IpV4Address => "<ipv4>",
            Rule::IpV6Address => "<ipv6>",
            Rule::Identifier => "<identifier>",
            Rule::Token => "<token>",
        }
    }

    /// One line, in terms of what the rule removes rather than how.
    pub fn description(self) -> &'static str {
        match self {
            Rule::CommandArguments => {
                "Every argument after the program name was removed from command lines."
            }
            Rule::HomePath => "The path of the user's home directory was replaced.",
            Rule::Username => "The user's login name was replaced.",
            Rule::Hostname => "The machine's hostname was replaced.",
            Rule::MacAddress => "Hardware addresses were replaced.",
            Rule::IpV4Address => "IPv4 addresses were replaced.",
            Rule::IpV6Address => "IPv6 addresses were replaced.",
            Rule::Identifier => "Machine identifiers recorded in the inventory were replaced.",
            Rule::Token => "Runs of characters shaped like a credential or key were replaced.",
        }
    }

    pub const ALL: [Rule; 9] = [
        Rule::CommandArguments,
        Rule::HomePath,
        Rule::Username,
        Rule::Hostname,
        Rule::MacAddress,
        Rule::IpV4Address,
        Rule::IpV6Address,
        Rule::Identifier,
        Rule::Token,
    ];
}

/// One rule's line in the report.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuleSummary {
    pub rule: String,
    pub description: String,
    pub replacements: u64,
}

/// What redaction did to a package, or would do to one.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RedactionReport {
    pub policy_version: u32,
    /// Strings examined. A reader comparing this with `replacements` can see
    /// how much of the package was touched at all.
    pub fields_scanned: u64,
    pub replacements: u64,
    pub rules: Vec<RuleSummary>,
    /// Data classes the export was asked not to include at all.
    pub withheld_data_classes: Vec<String>,
    /// A note about what redaction is not. It goes in the file rather than
    /// only in this comment, because the person reading the package is the one
    /// who needs to know.
    pub caveat: String,
}

impl RedactionReport {
    pub fn replacements_for(&self, rule: Rule) -> u64 {
        self.rules
            .iter()
            .find(|summary| summary.rule == rule.key())
            .map(|summary| summary.replacements)
            .unwrap_or(0)
    }

    pub fn rule_keys(&self) -> Vec<String> {
        self.rules
            .iter()
            .filter(|summary| summary.replacements > 0)
            .map(|summary| summary.rule.clone())
            .collect()
    }
}

/// The literal values and shape rules that apply to this machine.
#[derive(Clone, Debug, Default)]
pub struct Redactor {
    /// Longest first, so `/home/tim/Documents` is matched before `/home/tim`
    /// and `tim` never eats the middle of a longer replacement.
    literals: Vec<(String, Rule)>,
    counts: BTreeMap<Rule, u64>,
    fields_scanned: u64,
    withheld: Vec<String>,
}

impl Redactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the literal vocabulary from an inventory.
    ///
    /// Only entries the collector classified as personal or as identifiers are
    /// used. A kernel version is not redacted just because it happens to
    /// contain digits.
    pub fn from_inventory(inventory: &Inventory) -> Self {
        let mut redactor = Self::new();
        for (key, entry) in &inventory.entries {
            if entry.sensitivity == Sensitivity::Public || entry.value.trim().is_empty() {
                continue;
            }
            let rule = rule_for(key, entry.sensitivity);
            redactor.add_literal(&entry.value, rule);
        }
        redactor
    }

    /// Add a literal value that must never appear in the package.
    pub fn add_literal(&mut self, value: &str, rule: Rule) {
        let value = value.trim();
        // A one or two character "secret" would match inside half the words in
        // the file and would make the package unreadable without making it
        // safer.
        if value.len() < 3 {
            return;
        }
        if self.literals.iter().any(|(existing, _)| existing == value) {
            return;
        }
        self.literals.push((value.to_string(), rule));
        self.literals
            .sort_by(|left, right| right.0.len().cmp(&left.0.len()).then(left.0.cmp(&right.0)));
    }

    /// Record a data class the export was told not to include.
    pub fn withhold(&mut self, data_class: impl Into<String>) {
        let class = data_class.into();
        if !self.withheld.contains(&class) {
            self.withheld.push(class);
        }
    }

    fn count(&mut self, rule: Rule, times: u64) {
        if times > 0 {
            *self.counts.entry(rule).or_default() += times;
        }
    }

    /// Redact one free-text field.
    pub fn apply(&mut self, text: &str) -> String {
        self.fields_scanned += 1;
        let mut current = text.to_string();

        let literals = self.literals.clone();
        for (value, rule) in literals {
            let occurrences = current.matches(value.as_str()).count() as u64;
            if occurrences > 0 {
                current = current.replace(value.as_str(), rule.placeholder());
                self.count(rule, occurrences);
            }
        }

        let (replaced, hits) = scan_shapes(&current);
        for (rule, times) in hits {
            self.count(rule, times);
        }
        replaced
    }

    /// Redact an optional field, keeping `None` as `None`.
    pub fn apply_optional(&mut self, text: Option<&str>) -> Option<String> {
        text.map(|value| self.apply(value))
    }

    /// Redact a command line.
    ///
    /// The program name survives, because knowing that `ffmpeg` was running is
    /// the whole point of the field. Everything after it is dropped without
    /// being examined: arguments are where credentials, personal paths, and
    /// search terms live, and a scanner that got it right nine times out of ten
    /// would be worse than useless here.
    pub fn apply_command_line(&mut self, command_line: &str) -> String {
        self.fields_scanned += 1;
        let trimmed = command_line.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        // Collectors join `/proc/[pid]/cmdline` with spaces, and a NUL
        // separator can survive a badly behaved reader, so both split here.
        let mut parts = trimmed.splitn(2, [' ', '\0']);
        let program = parts.next().unwrap_or_default();
        let had_arguments = parts.next().is_some_and(|rest| !rest.trim().is_empty());

        // The program path itself can still carry a home directory or a
        // token-shaped build hash, so it goes through the ordinary rules.
        let program = {
            self.fields_scanned -= 1;
            self.apply(program)
        };
        if had_arguments {
            self.count(Rule::CommandArguments, 1);
            format!("{program} {}", Rule::CommandArguments.placeholder())
        } else {
            program
        }
    }

    pub fn report(&self) -> RedactionReport {
        let rules = Rule::ALL
            .iter()
            .map(|rule| RuleSummary {
                rule: rule.key().to_string(),
                description: rule.description().to_string(),
                replacements: self.counts.get(rule).copied().unwrap_or(0),
            })
            .collect();
        RedactionReport {
            policy_version: REDACTION_POLICY_VERSION,
            fields_scanned: self.fields_scanned,
            replacements: self.counts.values().sum(),
            rules,
            withheld_data_classes: self.withheld.clone(),
            caveat: "Redaction removes command-line arguments, known personal values, \
                     addresses, identifiers, and credential-shaped text. It cannot know that \
                     an ordinary-looking word is sensitive. Read this package before sending \
                     it to anyone."
                .to_string(),
        }
    }

    /// A redactor that has counted nothing yet but knows the same values.
    pub fn reset_counts(&self) -> Self {
        Self {
            literals: self.literals.clone(),
            counts: BTreeMap::new(),
            fields_scanned: 0,
            withheld: self.withheld.clone(),
        }
    }
}

fn rule_for(key: &str, sensitivity: Sensitivity) -> Rule {
    if key.ends_with(".mac") || key.contains("mac_address") {
        Rule::MacAddress
    } else if key.contains("home") {
        Rule::HomePath
    } else if key.contains("user") {
        Rule::Username
    } else if key.contains("host") {
        Rule::Hostname
    } else if sensitivity == Sensitivity::Identifier {
        Rule::Identifier
    } else {
        Rule::Username
    }
}

/// Replace everything that has the shape of an address or a credential.
///
/// Order matters. Addresses are matched before tokens, because an IPv6 address
/// and a hex key are both long runs of hex digits and only one of them should
/// be called a token.
fn scan_shapes(text: &str) -> (String, Vec<(Rule, u64)>) {
    let mut hits: BTreeMap<Rule, u64> = BTreeMap::new();
    let characters: Vec<char> = text.chars().collect();
    let mut output = String::with_capacity(text.len());
    let mut index = 0usize;

    while index < characters.len() {
        if let Some(length) = mac_at(&characters, index) {
            output.push_str(Rule::MacAddress.placeholder());
            *hits.entry(Rule::MacAddress).or_default() += 1;
            index += length;
            continue;
        }
        if let Some(length) = ipv6_at(&characters, index) {
            output.push_str(Rule::IpV6Address.placeholder());
            *hits.entry(Rule::IpV6Address).or_default() += 1;
            index += length;
            continue;
        }
        if let Some(length) = ipv4_at(&characters, index) {
            output.push_str(Rule::IpV4Address.placeholder());
            *hits.entry(Rule::IpV4Address).or_default() += 1;
            index += length;
            continue;
        }
        if let Some(length) = token_at(&characters, index) {
            output.push_str(Rule::Token.placeholder());
            *hits.entry(Rule::Token).or_default() += 1;
            index += length;
            continue;
        }
        output.push(characters[index]);
        index += 1;
    }

    (output, hits.into_iter().collect())
}

/// A match only counts when it starts at a boundary, so the tail of a longer
/// word is never mistaken for an address.
fn at_boundary(characters: &[char], index: usize) -> bool {
    index == 0
        || !characters[index - 1].is_ascii_alphanumeric() && characters[index - 1] != ':'
        || characters[index - 1] == ' '
}

fn mac_at(characters: &[char], start: usize) -> Option<usize> {
    if !at_boundary(characters, start) {
        return None;
    }
    // Six pairs of hex digits separated by colons or dashes.
    let mut index = start;
    for group in 0..6 {
        if group > 0 {
            let separator = *characters.get(index)?;
            if separator != ':' && separator != '-' {
                return None;
            }
            index += 1;
        }
        for _ in 0..2 {
            if !characters.get(index)?.is_ascii_hexdigit() {
                return None;
            }
            index += 1;
        }
    }
    if characters
        .get(index)
        .is_some_and(|next| next.is_ascii_hexdigit() || *next == ':' || *next == '-')
    {
        return None;
    }
    Some(index - start)
}

fn ipv4_at(characters: &[char], start: usize) -> Option<usize> {
    if !at_boundary(characters, start) {
        return None;
    }
    let mut index = start;
    for group in 0..4 {
        if group > 0 {
            if *characters.get(index)? != '.' {
                return None;
            }
            index += 1;
        }
        let digits_start = index;
        while characters.get(index).is_some_and(char::is_ascii_digit) {
            index += 1;
        }
        let digits = index - digits_start;
        if digits == 0 || digits > 3 {
            return None;
        }
        let value: u32 = characters[digits_start..index]
            .iter()
            .collect::<String>()
            .parse()
            .ok()?;
        if value > 255 {
            return None;
        }
    }
    if characters
        .get(index)
        .is_some_and(|next| next.is_ascii_alphanumeric() || *next == '.')
    {
        return None;
    }
    Some(index - start)
}

fn ipv6_at(characters: &[char], start: usize) -> Option<usize> {
    if !at_boundary(characters, start) {
        return None;
    }
    let mut index = start;
    let mut colons = 0usize;
    let mut hex = 0usize;
    while let Some(character) = characters.get(index) {
        if character.is_ascii_hexdigit() {
            hex += 1;
        } else if *character == ':' {
            colons += 1;
        } else {
            break;
        }
        index += 1;
    }
    // Two colons is a MAC-with-two-groups or a time; a real address has at
    // least two and usually seven. Requiring three keeps `12:34:56` out.
    if colons < 3 || hex == 0 {
        return None;
    }
    Some(index - start)
}

fn token_at(characters: &[char], start: usize) -> Option<usize> {
    if !at_boundary(characters, start) {
        return None;
    }
    let mut index = start;
    let mut has_digit = false;
    let mut has_letter = false;
    while let Some(character) = characters.get(index) {
        if character.is_ascii_alphanumeric() || *character == '_' || *character == '-' {
            has_digit |= character.is_ascii_digit();
            has_letter |= character.is_ascii_alphabetic();
            index += 1;
        } else {
            break;
        }
    }
    let length = index - start;
    // Both classes have to be present. A long run of only letters is a word,
    // and a long run of only digits is a counter.
    if length >= MINIMUM_TOKEN_LENGTH && has_digit && has_letter {
        Some(length)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_store::InventoryEntry;

    fn inventory() -> Inventory {
        let mut inventory = Inventory::new(1_000);
        inventory
            .insert("os.name", InventoryEntry::public("Zorin OS 18"))
            .insert("session.user", InventoryEntry::personal("tim"))
            .insert("session.home", InventoryEntry::personal("/home/tim"))
            .insert("host.name", InventoryEntry::personal("workshop"))
            .insert(
                "network.eth0.mac",
                InventoryEntry::identifier("aa:bb:cc:dd:ee:ff"),
            )
            .insert("machine.id", InventoryEntry::identifier("9f2c4d1e8a7b"));
        inventory
    }

    fn redactor() -> Redactor {
        Redactor::from_inventory(&inventory())
    }

    #[test]
    fn a_public_inventory_value_is_left_alone() {
        let mut redactor = redactor();
        assert_eq!(redactor.apply("Zorin OS 18"), "Zorin OS 18");
        assert_eq!(redactor.report().replacements, 0);
    }

    #[test]
    fn the_home_directory_is_replaced_before_the_username_inside_it() {
        let mut redactor = redactor();
        let result = redactor.apply("/home/tim/Videos/holiday.mp4");
        assert_eq!(result, "<home>/Videos/holiday.mp4");
        assert!(!result.contains("tim"));
        assert_eq!(redactor.report().replacements_for(Rule::HomePath), 1);
        assert_eq!(redactor.report().replacements_for(Rule::Username), 0);
    }

    #[test]
    fn the_hostname_and_username_are_replaced_wherever_they_appear() {
        let mut redactor = redactor();
        let result = redactor.apply("tim@workshop ran a build");
        assert!(!result.contains("tim"));
        assert!(!result.contains("workshop"));
        assert_eq!(redactor.report().replacements_for(Rule::Username), 1);
        assert_eq!(redactor.report().replacements_for(Rule::Hostname), 1);
    }

    #[test]
    fn a_hardware_address_is_replaced_even_when_the_inventory_never_saw_it() {
        let mut redactor = Redactor::new();
        let result = redactor.apply("link up on 11:22:33:44:55:66");
        assert_eq!(result, "link up on <mac>");
        assert_eq!(redactor.report().replacements_for(Rule::MacAddress), 1);
    }

    #[test]
    fn addresses_of_both_families_are_replaced() {
        let mut redactor = Redactor::new();
        assert_eq!(
            redactor.apply("peer 192.168.1.44 replied"),
            "peer <ipv4> replied"
        );
        assert_eq!(
            redactor.apply("peer fe80::1c2d:3e4f:5a6b:7c8d replied"),
            "peer <ipv6> replied"
        );
        let report = redactor.report();
        assert_eq!(report.replacements_for(Rule::IpV4Address), 1);
        assert_eq!(report.replacements_for(Rule::IpV6Address), 1);
    }

    #[test]
    fn a_version_number_is_not_mistaken_for_an_address() {
        let mut redactor = Redactor::new();
        assert_eq!(redactor.apply("kernel 6.11.0"), "kernel 6.11.0");
        assert_eq!(redactor.apply("999.1.1.1"), "999.1.1.1");
        assert_eq!(redactor.report().replacements, 0);
    }

    #[test]
    fn a_credential_shaped_run_is_replaced_wherever_it_appears() {
        let mut redactor = Redactor::new();
        let result = redactor.apply("Authorization ghp_S3cretT0kenValue000001 sent");
        assert_eq!(result, "Authorization <token> sent");
        assert_eq!(redactor.report().replacements_for(Rule::Token), 1);
    }

    #[test]
    fn an_ordinary_long_word_is_not_treated_as_a_credential() {
        let mut redactor = Redactor::new();
        for ordinary in [
            "process.cpu.utilization",
            "NetworkManager",
            "org.freedesktop.systemd1",
            "internationalization",
        ] {
            assert_eq!(redactor.apply(ordinary), ordinary, "{ordinary}");
        }
        assert_eq!(redactor.report().replacements, 0);
    }

    #[test]
    fn command_arguments_are_dropped_whole_and_the_program_survives() {
        let mut redactor = redactor();
        let result = redactor.apply_command_line(
            "/usr/bin/curl -H 'Authorization: Bearer abc' https://example.test",
        );
        assert_eq!(result, "/usr/bin/curl [arguments withheld]");
        assert_eq!(
            redactor.report().replacements_for(Rule::CommandArguments),
            1
        );
    }

    #[test]
    fn a_command_line_with_no_arguments_keeps_its_program_and_reports_nothing() {
        let mut redactor = redactor();
        assert_eq!(
            redactor.apply_command_line("/usr/bin/gedit"),
            "/usr/bin/gedit"
        );
        assert_eq!(
            redactor.report().replacements_for(Rule::CommandArguments),
            0
        );
    }

    #[test]
    fn a_program_path_inside_the_home_directory_is_still_redacted() {
        let mut redactor = redactor();
        let result = redactor.apply_command_line("/home/tim/.local/bin/tool --flag");
        assert_eq!(result, "<home>/.local/bin/tool [arguments withheld]");
        assert!(!result.contains("tim"));
    }

    #[test]
    fn a_nul_separated_command_line_is_split_the_same_way() {
        let mut redactor = Redactor::new();
        assert_eq!(
            redactor.apply_command_line("/usr/bin/ssh\u{0}user@host"),
            "/usr/bin/ssh [arguments withheld]"
        );
    }

    #[test]
    fn an_empty_command_line_produces_an_empty_string_rather_than_a_placeholder() {
        let mut redactor = Redactor::new();
        assert_eq!(redactor.apply_command_line("   "), "");
    }

    #[test]
    fn the_report_names_every_rule_even_the_ones_that_matched_nothing() {
        let report = redactor().report();
        assert_eq!(report.rules.len(), Rule::ALL.len());
        assert_eq!(report.policy_version, REDACTION_POLICY_VERSION);
        assert!(report.rules.iter().all(|rule| !rule.description.is_empty()));
        assert!(report.caveat.contains("cannot know"));
    }

    #[test]
    fn a_withheld_data_class_is_named_in_the_report() {
        let mut redactor = Redactor::new();
        redactor.withhold("process_command_lines");
        redactor.withhold("process_command_lines");
        assert_eq!(
            redactor.report().withheld_data_classes,
            vec!["process_command_lines".to_string()]
        );
    }

    #[test]
    fn an_optional_field_keeps_its_absence() {
        let mut redactor = redactor();
        assert_eq!(redactor.apply_optional(None), None);
        assert_eq!(
            redactor.apply_optional(Some("/home/tim")),
            Some("<home>".to_string())
        );
    }

    #[test]
    fn a_very_short_inventory_value_is_never_used_as_a_literal() {
        let mut inventory = Inventory::new(1);
        inventory.insert("session.user", InventoryEntry::personal("ab"));
        let mut redactor = Redactor::from_inventory(&inventory);
        // Otherwise every "ab" in the package would be replaced, which would
        // destroy the file without protecting anything.
        assert_eq!(redactor.apply("a stable label"), "a stable label");
    }

    #[test]
    fn resetting_keeps_the_vocabulary_and_clears_the_tally() {
        let mut first = redactor();
        first.apply("/home/tim");
        assert_eq!(first.report().replacements, 1);
        let mut second = first.reset_counts();
        assert_eq!(second.report().replacements, 0);
        assert_eq!(second.apply("/home/tim"), "<home>");
    }
}
