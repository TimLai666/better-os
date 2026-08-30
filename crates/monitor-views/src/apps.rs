//! The Apps view: what a group costs, and how much of that is actually known.
//!
//! An application's CPU total is a sum over its processes, and a sum is only
//! as good as its terms. If one of six processes will not report its resident
//! memory, the honest answer is not the total of the other five presented as
//! if it were complete. [`Aggregate`] therefore carries the count it summed
//! and the count it could not, and the view says so.

use crate::facts::ProcessFacts;
use crate::field::Field;
use crate::grouping::{AppGroup, Grouping, GroupingPrecedence, group_processes};
use std::collections::HashMap;

/// A sum over a group's processes, with its own coverage attached.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aggregate {
    pub total: f64,
    /// Processes that contributed a real value.
    pub counted: usize,
    /// Processes that had no value to contribute.
    pub missing: usize,
}

impl Aggregate {
    /// Every member reported, so the total is the whole story.
    pub fn is_complete(&self) -> bool {
        self.missing == 0 && self.counted > 0
    }

    /// Some members reported and some did not: a floor, not a total.
    pub fn is_partial(&self) -> bool {
        self.counted > 0 && self.missing > 0
    }

    /// Nothing reported. There is no number to show at all.
    pub fn is_unavailable(&self) -> bool {
        self.counted == 0
    }

    /// The value, only when it is the whole sum.
    pub fn complete_value(&self) -> Option<f64> {
        self.is_complete().then_some(self.total)
    }
}

fn aggregate<'a, T: Copy + 'a>(
    members: impl Iterator<Item = &'a ProcessFacts>,
    read: impl Fn(&ProcessFacts) -> &Field<T>,
    widen: impl Fn(T) -> f64,
) -> Aggregate {
    let mut result = Aggregate::default();
    for member in members {
        // A stale reading counts, because it was measured; the group's own
        // freshness is a separate question the round already answers.
        match read(member).any_value() {
            Some(value) => {
                result.total += widen(*value);
                result.counted += 1;
            }
            None => result.missing += 1,
        }
    }
    result
}

/// One row of the Apps view.
#[derive(Clone, Debug, PartialEq)]
pub struct AppRow {
    pub group: AppGroup,
    pub cpu_utilization: Aggregate,
    pub memory_resident: Aggregate,
    pub memory_swap: Aggregate,
    pub read_rate: Aggregate,
    pub write_rate: Aggregate,
    /// Indices into the model's process list, in the order the detail view
    /// should show them.
    pub member_indices: Vec<usize>,
}

impl AppRow {
    pub fn process_count(&self) -> usize {
        self.group.members.len()
    }
}

/// How the Apps view is ordered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppSort {
    Name,
    Cpu,
    Memory,
}

/// The Apps view: user-facing applications and background services, kept
/// apart, each expandable to its processes.
pub struct AppsModel {
    processes: Vec<ProcessFacts>,
    precedence: GroupingPrecedence,
    grouping: Grouping,
    applications: Vec<AppRow>,
    services: Vec<AppRow>,
    expanded: Vec<String>,
    sort: AppSort,
    filter: String,
}

impl AppsModel {
    pub fn new(processes: Vec<ProcessFacts>, precedence: GroupingPrecedence) -> Self {
        let mut model = Self {
            processes,
            precedence,
            grouping: Grouping::default(),
            applications: Vec::new(),
            services: Vec::new(),
            expanded: Vec::new(),
            sort: AppSort::Cpu,
            filter: String::new(),
        };
        model.rebuild();
        model
    }

    /// Adopt a newer round. Expansion, sort, and filter survive, keyed by the
    /// stable group key rather than by row position.
    pub fn update(&mut self, processes: Vec<ProcessFacts>) {
        self.processes = processes;
        self.rebuild();
    }

    pub fn processes(&self) -> &[ProcessFacts] {
        &self.processes
    }

    pub fn grouping(&self) -> &Grouping {
        &self.grouping
    }

    pub fn applications(&self) -> &[AppRow] {
        &self.applications
    }

    pub fn services(&self) -> &[AppRow] {
        &self.services
    }

    pub fn sort(&self) -> AppSort {
        self.sort
    }

    pub fn set_sort(&mut self, sort: AppSort) {
        self.sort = sort;
        self.rebuild();
    }

    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.rebuild();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn is_expanded(&self, key: &str) -> bool {
        self.expanded.iter().any(|expanded| expanded == key)
    }

    pub fn toggle_expanded(&mut self, key: &str) {
        match self.expanded.iter().position(|expanded| expanded == key) {
            Some(index) => {
                self.expanded.remove(index);
            }
            None => self.expanded.push(key.to_string()),
        }
    }

    pub fn set_precedence(&mut self, precedence: GroupingPrecedence) {
        self.precedence = precedence;
        self.rebuild();
    }

    pub fn process(&self, index: usize) -> Option<&ProcessFacts> {
        self.processes.get(index)
    }

    fn rebuild(&mut self) {
        self.grouping = group_processes(&self.processes, &self.precedence);
        let position: HashMap<u32, usize> = self
            .processes
            .iter()
            .enumerate()
            .map(|(index, process)| (process.pid, index))
            .collect();

        let needle = self.filter.trim().to_lowercase();
        let build = |groups: &[AppGroup]| -> Vec<AppRow> {
            groups
                .iter()
                .filter(|group| {
                    needle.is_empty() || group.display_name.to_lowercase().contains(&needle)
                })
                .map(|group| {
                    let mut member_indices: Vec<usize> = group
                        .members
                        .iter()
                        .filter_map(|member| position.get(&member.pid).copied())
                        .collect();
                    member_indices.sort_by(|left, right| {
                        let a = &self.processes[*left];
                        let b = &self.processes[*right];
                        b.cpu_utilization
                            .any_value()
                            .copied()
                            .unwrap_or(f64::NEG_INFINITY)
                            .partial_cmp(
                                &a.cpu_utilization
                                    .any_value()
                                    .copied()
                                    .unwrap_or(f64::NEG_INFINITY),
                            )
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.pid.cmp(&b.pid))
                    });
                    let members = || member_indices.iter().map(|index| &self.processes[*index]);
                    AppRow {
                        cpu_utilization: aggregate(
                            members(),
                            |process| &process.cpu_utilization,
                            |value| value,
                        ),
                        memory_resident: aggregate(
                            members(),
                            |process| &process.memory_resident,
                            |value| value as f64,
                        ),
                        memory_swap: aggregate(
                            members(),
                            |process| &process.memory_swap,
                            |value| value as f64,
                        ),
                        read_rate: aggregate(
                            members(),
                            |process| &process.read_rate,
                            |value| value,
                        ),
                        write_rate: aggregate(
                            members(),
                            |process| &process.write_rate,
                            |value| value,
                        ),
                        group: group.clone(),
                        member_indices,
                    }
                })
                .collect()
        };

        self.applications = build(&self.grouping.applications);
        self.services = build(&self.grouping.services);
        let sort = self.sort;
        for rows in [&mut self.applications, &mut self.services] {
            rows.sort_by(|a, b| {
                match sort {
                    AppSort::Name => a
                        .group
                        .display_name
                        .to_lowercase()
                        .cmp(&b.group.display_name.to_lowercase()),
                    // A group with nothing measured sorts last rather than as a
                    // zero, the same rule the process table follows.
                    AppSort::Cpu => descending_by_total(&a.cpu_utilization, &b.cpu_utilization),
                    AppSort::Memory => descending_by_total(&a.memory_resident, &b.memory_resident),
                }
                .then_with(|| a.group.key.cmp(&b.group.key))
            });
        }

        // An expansion whose group is gone would otherwise grow without bound
        // across a long session.
        let live: Vec<String> = self
            .applications
            .iter()
            .chain(self.services.iter())
            .map(|row| row.group.key.clone())
            .collect();
        self.expanded.retain(|key| live.contains(key));
    }

    /// The processes of the busiest groups, for the Overview's top-apps list.
    pub fn top_by_cpu(&self, limit: usize) -> Vec<&AppRow> {
        let mut rows: Vec<&AppRow> = self
            .applications
            .iter()
            .chain(self.services.iter())
            .collect();
        rows.sort_by(|a, b| {
            descending_by_total(&a.cpu_utilization, &b.cpu_utilization)
                .then_with(|| a.group.key.cmp(&b.group.key))
        });
        rows.into_iter().take(limit).collect()
    }
}

/// Order two aggregates largest first, with "nothing measured" always last.
///
/// The two rules pull in opposite directions, which is why this is not a
/// reversed ascending comparison: reversing would send the unmeasured groups
/// to the top of the list of the busiest applications.
fn descending_by_total(left: &Aggregate, right: &Aggregate) -> std::cmp::Ordering {
    match (left.is_unavailable(), right.is_unavailable()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        (false, false) => right
            .total
            .partial_cmp(&left.total)
            .unwrap_or(std::cmp::Ordering::Equal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::{UnknownReason, UnsupportedReason};

    const NAUTILUS: &str = "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.gnome.Nautilus-4321.scope";
    const NETWORK_MANAGER: &str = "/system.slice/NetworkManager.service";

    fn process(
        pid: u32,
        name: &str,
        parent: u32,
        cgroup: &str,
        cpu: f64,
        rss: u64,
    ) -> ProcessFacts {
        let mut facts = ProcessFacts::synthetic(pid, name);
        facts.parent_pid = Field::Value(parent as u64);
        facts.cgroup = Field::Value(cgroup.to_string());
        facts.cpu_utilization = Field::Value(cpu);
        facts.memory_resident = Field::Value(rss);
        facts
    }

    fn model() -> AppsModel {
        AppsModel::new(
            vec![
                process(100, "nautilus", 1, NAUTILUS, 0.2, 100),
                process(101, "nautilus", 100, NAUTILUS, 0.1, 50),
                process(200, "NetworkManager", 1, NETWORK_MANAGER, 0.01, 20),
            ],
            GroupingPrecedence::default(),
        )
    }

    #[test]
    fn an_application_totals_its_processes_and_services_stay_separate() {
        let model = model();
        assert_eq!(model.applications().len(), 1);
        assert_eq!(model.services().len(), 1);

        let app = &model.applications()[0];
        assert_eq!(app.group.display_name, "Nautilus");
        assert_eq!(app.process_count(), 2);
        assert!(app.cpu_utilization.is_complete());
        assert!((app.cpu_utilization.total - 0.3).abs() < 1e-9);
        assert_eq!(app.memory_resident.complete_value(), Some(150.0));
    }

    #[test]
    fn a_member_with_no_reading_makes_the_total_partial_rather_than_wrong() {
        let mut processes = vec![
            process(100, "nautilus", 1, NAUTILUS, 0.2, 100),
            process(101, "nautilus", 100, NAUTILUS, 0.1, 50),
        ];
        processes[1].memory_resident = Field::Unsupported(UnsupportedReason::NotReported {
            detail: "kernel thread".into(),
        });
        let model = AppsModel::new(processes, GroupingPrecedence::default());
        let app = &model.applications()[0];
        assert!(app.memory_resident.is_partial());
        assert_eq!(app.memory_resident.counted, 1);
        assert_eq!(app.memory_resident.missing, 1);
        assert_eq!(
            app.memory_resident.complete_value(),
            None,
            "a partial sum must not be offered as a total"
        );
    }

    #[test]
    fn a_group_where_nothing_was_measured_reports_unavailable_not_zero() {
        let mut only = process(100, "nautilus", 1, NAUTILUS, 0.0, 0);
        only.read_rate = Field::Unknown(UnknownReason::NotYetSampled);
        let model = AppsModel::new(vec![only], GroupingPrecedence::default());
        let app = &model.applications()[0];
        assert!(app.read_rate.is_unavailable());
        assert_eq!(app.read_rate.total, 0.0);
        assert!(!app.read_rate.is_complete());
        // A measured zero is still complete.
        assert!(app.write_rate.is_complete());
    }

    #[test]
    fn members_are_listed_with_the_busiest_process_first() {
        let model = model();
        let app = &model.applications()[0];
        let pids: Vec<u32> = app
            .member_indices
            .iter()
            .map(|index| model.process(*index).unwrap().pid)
            .collect();
        assert_eq!(pids, vec![100, 101]);
    }

    #[test]
    fn expansion_survives_a_new_round_and_is_dropped_when_the_app_exits() {
        let mut model = model();
        let key = model.applications()[0].group.key.clone();
        model.toggle_expanded(&key);
        assert!(model.is_expanded(&key));

        model.update(vec![process(100, "nautilus", 1, NAUTILUS, 0.2, 100)]);
        assert!(model.is_expanded(&key), "the key is stable across rounds");

        model.update(vec![process(
            200,
            "NetworkManager",
            1,
            NETWORK_MANAGER,
            0.01,
            20,
        )]);
        assert!(
            !model.is_expanded(&key),
            "an app that exited is not expanded"
        );
    }

    #[test]
    fn sorting_by_cpu_puts_a_group_with_nothing_measured_last() {
        let mut unmeasured = process(300, "ghost", 1, "/system.slice/ghost.service", 0.0, 0);
        unmeasured.cpu_utilization = Field::Unknown(UnknownReason::NotYetSampled);
        let mut processes = vec![unmeasured];
        processes.push(process(200, "NetworkManager", 1, NETWORK_MANAGER, 0.01, 20));
        let model = AppsModel::new(processes, GroupingPrecedence::default());
        let names: Vec<&str> = model
            .services()
            .iter()
            .map(|row| row.group.display_name.as_str())
            .collect();
        assert_eq!(names, vec!["NetworkManager", "ghost"]);
    }

    #[test]
    fn the_filter_matches_the_application_name() {
        let mut model = model();
        model.set_filter("nauti");
        assert_eq!(model.applications().len(), 1);
        assert!(model.services().is_empty());
        model.set_filter("nothing-here");
        assert!(model.applications().is_empty());
    }

    #[test]
    fn the_top_list_ranks_applications_and_services_together() {
        let model = model();
        let top = model.top_by_cpu(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].group.display_name, "Nautilus");
        assert_eq!(top[1].group.display_name, "NetworkManager");
    }

    #[test]
    fn sorting_by_name_is_stable_and_case_insensitive() {
        let mut model = model();
        model.set_sort(AppSort::Name);
        assert_eq!(model.sort(), AppSort::Name);
        assert_eq!(model.applications()[0].group.display_name, "Nautilus");
    }
}
