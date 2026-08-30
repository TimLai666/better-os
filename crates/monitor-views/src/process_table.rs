//! The Processes view, as a model that does not need a window.
//!
//! Everything the table does that could be wrong — which rows are visible,
//! what order they are in, where a row sits in the tree, and how a row without
//! a value sorts — is decided here and tested without GPUI. The renderer's
//! only job is to draw `visible_rows()`.
//!
//! Two rules are worth stating because they are easy to get backwards.
//!
//! A row whose value is missing sorts last in both directions. Sorting by CPU
//! descending must not put "unknown" at the top of the list of the busiest
//! processes, and sorting ascending must not put it at the top of the quietest
//! either. Missing is not a position on the scale.
//!
//! The filter never searches a hidden command line. If the privacy toggle is
//! off the command line is not on screen, and a filter that could still match
//! it would let anyone confirm the contents of something they were not shown.

use crate::facts::ProcessFacts;
use crate::field::Field;
use std::cmp::Ordering;
use std::collections::HashMap;

/// The columns Issue #16 asks for, restricted to what ticket 22's collectors
/// can actually produce on Linux.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProcessColumn {
    Name,
    Pid,
    ParentPid,
    User,
    State,
    CpuUtilization,
    CpuTime,
    Memory,
    Swap,
    ReadRate,
    WriteRate,
    Threads,
    FileDescriptors,
    StartTime,
    Nice,
    Cgroup,
    CommandLine,
}

impl ProcessColumn {
    /// Every column, in the order the table lays them out.
    pub const ALL: [ProcessColumn; 17] = [
        ProcessColumn::Name,
        ProcessColumn::Pid,
        ProcessColumn::ParentPid,
        ProcessColumn::User,
        ProcessColumn::State,
        ProcessColumn::CpuUtilization,
        ProcessColumn::CpuTime,
        ProcessColumn::Memory,
        ProcessColumn::Swap,
        ProcessColumn::ReadRate,
        ProcessColumn::WriteRate,
        ProcessColumn::Threads,
        ProcessColumn::FileDescriptors,
        ProcessColumn::StartTime,
        ProcessColumn::Nice,
        ProcessColumn::Cgroup,
        ProcessColumn::CommandLine,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ProcessColumn::Name => "name",
            ProcessColumn::Pid => "pid",
            ProcessColumn::ParentPid => "ppid",
            ProcessColumn::User => "user",
            ProcessColumn::State => "state",
            ProcessColumn::CpuUtilization => "cpu",
            ProcessColumn::CpuTime => "cpu-time",
            ProcessColumn::Memory => "memory",
            ProcessColumn::Swap => "swap",
            ProcessColumn::ReadRate => "read",
            ProcessColumn::WriteRate => "write",
            ProcessColumn::Threads => "threads",
            ProcessColumn::FileDescriptors => "descriptors",
            ProcessColumn::StartTime => "start-time",
            ProcessColumn::Nice => "nice",
            ProcessColumn::Cgroup => "cgroup",
            ProcessColumn::CommandLine => "command-line",
        }
    }

    /// Numeric columns start descending, because the first question about a
    /// resource column is always which process is using the most.
    pub fn starts_descending(self) -> bool {
        !matches!(
            self,
            ProcessColumn::Name
                | ProcessColumn::User
                | ProcessColumn::State
                | ProcessColumn::Cgroup
                | ProcessColumn::CommandLine
        )
    }

    /// The column that only appears when the user has turned command lines on.
    pub fn is_private(self) -> bool {
        matches!(self, ProcessColumn::CommandLine)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn flipped(self) -> Self {
        match self {
            SortDirection::Ascending => SortDirection::Descending,
            SortDirection::Descending => SortDirection::Ascending,
        }
    }
}

/// A row as the renderer needs it: which process, and how deep in the tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VisibleRow {
    /// Index into the model's process list.
    pub index: usize,
    /// Zero in flat mode; the depth below a root in tree mode.
    pub depth: u16,
    /// Whether this row has children that the tree is showing under it.
    pub has_children: bool,
}

/// A sortable, filterable, optionally hierarchical process table.
pub struct ProcessTableModel {
    processes: Vec<ProcessFacts>,
    sort_column: ProcessColumn,
    sort_direction: SortDirection,
    filter: String,
    tree_mode: bool,
    show_command_line: bool,
    visible: Vec<VisibleRow>,
}

impl ProcessTableModel {
    pub fn new(processes: Vec<ProcessFacts>) -> Self {
        let mut model = Self {
            processes,
            sort_column: ProcessColumn::CpuUtilization,
            sort_direction: SortDirection::Descending,
            filter: String::new(),
            tree_mode: false,
            show_command_line: false,
            visible: Vec::new(),
        };
        model.rebuild();
        model
    }

    /// Replace the process list with a newer round, keeping the sort, filter,
    /// and tree settings. This is what a sampling tick calls.
    pub fn update(&mut self, processes: Vec<ProcessFacts>) {
        self.processes = processes;
        self.rebuild();
    }

    pub fn processes(&self) -> &[ProcessFacts] {
        &self.processes
    }

    pub fn visible_rows(&self) -> &[VisibleRow] {
        &self.visible
    }

    pub fn row(&self, index: usize) -> Option<&ProcessFacts> {
        self.processes.get(index)
    }

    /// The process behind a visible row position.
    pub fn process_at(&self, position: usize) -> Option<&ProcessFacts> {
        self.visible
            .get(position)
            .and_then(|row| self.processes.get(row.index))
    }

    pub fn sort(&self) -> (ProcessColumn, SortDirection) {
        (self.sort_column, self.sort_direction)
    }

    /// Sort by a column. Asking for the column that is already active flips
    /// the direction, which is what clicking a header twice should do.
    pub fn sort_by(&mut self, column: ProcessColumn) {
        if self.sort_column == column {
            self.sort_direction = self.sort_direction.flipped();
        } else {
            self.sort_column = column;
            self.sort_direction = if column.starts_descending() {
                SortDirection::Descending
            } else {
                SortDirection::Ascending
            };
        }
        self.rebuild();
    }

    pub fn set_sort(&mut self, column: ProcessColumn, direction: SortDirection) {
        self.sort_column = column;
        self.sort_direction = direction;
        self.rebuild();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.rebuild();
    }

    pub fn tree_mode(&self) -> bool {
        self.tree_mode
    }

    pub fn set_tree_mode(&mut self, tree_mode: bool) {
        self.tree_mode = tree_mode;
        self.rebuild();
    }

    pub fn show_command_line(&self) -> bool {
        self.show_command_line
    }

    pub fn set_show_command_line(&mut self, show: bool) {
        self.show_command_line = show;
        self.rebuild();
    }

    /// The columns to draw, which is every column minus the private ones the
    /// user has not opted into.
    pub fn columns(&self) -> Vec<ProcessColumn> {
        ProcessColumn::ALL
            .into_iter()
            .filter(|column| self.show_command_line || !column.is_private())
            .collect()
    }

    fn rebuild(&mut self) {
        let matching = self.matching_indices();
        self.visible = if self.tree_mode {
            self.build_tree(&matching)
        } else {
            let mut order = matching;
            self.sort_indices(&mut order);
            order
                .into_iter()
                .map(|index| VisibleRow {
                    index,
                    depth: 0,
                    has_children: false,
                })
                .collect()
        };
    }

    /// Indices of the processes the filter keeps.
    ///
    /// In tree mode an ancestor of a match is kept too, so a matching row is
    /// never orphaned under a parent that was filtered away.
    fn matching_indices(&self) -> Vec<usize> {
        if self.filter.trim().is_empty() {
            return (0..self.processes.len()).collect();
        }
        let needle = self.filter.trim().to_lowercase();
        let mut keep: Vec<bool> = self
            .processes
            .iter()
            .map(|process| self.matches(process, &needle))
            .collect();

        if self.tree_mode {
            let position: HashMap<u32, usize> = self
                .processes
                .iter()
                .enumerate()
                .map(|(index, process)| (process.pid, index))
                .collect();
            for index in 0..self.processes.len() {
                if !keep[index] {
                    continue;
                }
                let mut cursor = self.processes[index].parent();
                let mut hops = 0usize;
                while let Some(parent) = cursor {
                    let Some(&parent_index) = position.get(&parent) else {
                        break;
                    };
                    hops += 1;
                    if keep[parent_index] || hops > self.processes.len() {
                        break;
                    }
                    keep[parent_index] = true;
                    cursor = self.processes[parent_index].parent();
                }
            }
        }

        keep.iter()
            .enumerate()
            .filter_map(|(index, keep)| keep.then_some(index))
            .collect()
    }

    fn matches(&self, process: &ProcessFacts, needle: &str) -> bool {
        if process.pid.to_string().contains(needle) {
            return true;
        }
        if process.display_name().to_lowercase().contains(needle) {
            return true;
        }
        if process
            .user
            .any_value()
            .is_some_and(|user| user.to_lowercase().contains(needle))
        {
            return true;
        }
        // Only searchable when it is also visible.
        self.show_command_line
            && process
                .command_line
                .any_value()
                .is_some_and(|command| command.to_lowercase().contains(needle))
    }

    fn sort_indices(&self, indices: &mut [usize]) {
        let column = self.sort_column;
        let descending = self.sort_direction == SortDirection::Descending;
        indices.sort_by(|left, right| {
            let a = &self.processes[*left];
            let b = &self.processes[*right];
            let ordering = compare(a, b, column, descending);
            // PID breaks every tie, so the order is total and a refresh does
            // not shuffle rows that compare equal.
            ordering.then_with(|| a.pid.cmp(&b.pid))
        });
    }

    fn build_tree(&self, matching: &[usize]) -> Vec<VisibleRow> {
        let included: HashMap<u32, usize> = matching
            .iter()
            .map(|index| (self.processes[*index].pid, *index))
            .collect();

        let mut children: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut roots: Vec<usize> = Vec::new();
        for index in matching {
            let process = &self.processes[*index];
            match process.parent().filter(|parent| {
                // A process whose parent is not in view is a root here, and a
                // process that claims itself as its parent is not a cycle the
                // tree has to represent.
                *parent != process.pid && included.contains_key(parent)
            }) {
                Some(parent) => children.entry(parent).or_default().push(*index),
                None => roots.push(*index),
            }
        }

        self.sort_indices(&mut roots);
        for list in children.values_mut() {
            self.sort_indices(list);
        }

        let mut rows = Vec::with_capacity(matching.len());
        let mut visited: Vec<bool> = vec![false; self.processes.len()];
        // An explicit stack rather than recursion: a deep process tree must
        // not be able to overflow the render thread's stack.
        let mut stack: Vec<(usize, u16)> = roots.into_iter().rev().map(|i| (i, 0)).collect();
        while let Some((index, depth)) = stack.pop() {
            if visited[index] {
                continue;
            }
            visited[index] = true;
            let pid = self.processes[index].pid;
            let kids = children.get(&pid);
            rows.push(VisibleRow {
                index,
                depth,
                has_children: kids.is_some_and(|kids| !kids.is_empty()),
            });
            if let Some(kids) = kids {
                for child in kids.iter().rev() {
                    stack.push((*child, depth.saturating_add(1)));
                }
            }
        }

        // A parent cycle would leave rows unvisited. They still belong on
        // screen, so they are appended as roots rather than dropped.
        let mut orphans: Vec<usize> = matching
            .iter()
            .copied()
            .filter(|index| !visited[*index])
            .collect();
        self.sort_indices(&mut orphans);
        rows.extend(orphans.into_iter().map(|index| VisibleRow {
            index,
            depth: 0,
            has_children: false,
        }));
        rows
    }
}

/// Order two processes by one column.
///
/// A missing value always sorts after a present one, whichever direction the
/// column is in.
fn compare(
    a: &ProcessFacts,
    b: &ProcessFacts,
    column: ProcessColumn,
    descending: bool,
) -> Ordering {
    /// One numeric comparison for every numeric column. The columns carry
    /// three different integer widths and a float, so they arrive here already
    /// widened rather than through four near-identical functions.
    fn numeric(left: Option<f64>, right: Option<f64>, descending: bool) -> Ordering {
        order_optional(left, right, descending, |a, b| {
            a.partial_cmp(b).unwrap_or(Ordering::Equal)
        })
    }

    fn as_f64<T: Copy>(field: &Field<T>, widen: impl Fn(T) -> f64) -> Option<f64> {
        field.any_value().map(|value| widen(*value))
    }

    fn textual(left: &Field<String>, right: &Field<String>, descending: bool) -> Ordering {
        order_optional(
            left.any_value().map(|value| value.to_lowercase()),
            right.any_value().map(|value| value.to_lowercase()),
            descending,
            |a, b| a.cmp(b),
        )
    }

    match column {
        ProcessColumn::Name => order_optional(
            Some(a.display_name().to_lowercase()),
            Some(b.display_name().to_lowercase()),
            descending,
            |a, b| a.cmp(b),
        ),
        ProcessColumn::Pid => {
            let ordering = a.pid.cmp(&b.pid);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        }
        ProcessColumn::ParentPid => numeric(
            as_f64(&a.parent_pid, |v| v as f64),
            as_f64(&b.parent_pid, |v| v as f64),
            descending,
        ),
        ProcessColumn::User => textual(&a.user, &b.user, descending),
        ProcessColumn::State => textual(&a.state, &b.state, descending),
        ProcessColumn::CpuUtilization => numeric(
            as_f64(&a.cpu_utilization, |v| v),
            as_f64(&b.cpu_utilization, |v| v),
            descending,
        ),
        ProcessColumn::CpuTime => numeric(
            as_f64(&a.cpu_time_total, |v| v),
            as_f64(&b.cpu_time_total, |v| v),
            descending,
        ),
        ProcessColumn::Memory => numeric(
            as_f64(&a.memory_resident, |v| v as f64),
            as_f64(&b.memory_resident, |v| v as f64),
            descending,
        ),
        ProcessColumn::Swap => numeric(
            as_f64(&a.memory_swap, |v| v as f64),
            as_f64(&b.memory_swap, |v| v as f64),
            descending,
        ),
        ProcessColumn::ReadRate => numeric(
            as_f64(&a.read_rate, |v| v),
            as_f64(&b.read_rate, |v| v),
            descending,
        ),
        ProcessColumn::WriteRate => numeric(
            as_f64(&a.write_rate, |v| v),
            as_f64(&b.write_rate, |v| v),
            descending,
        ),
        ProcessColumn::Threads => numeric(
            as_f64(&a.threads, |v| v as f64),
            as_f64(&b.threads, |v| v as f64),
            descending,
        ),
        ProcessColumn::FileDescriptors => numeric(
            as_f64(&a.file_descriptors, |v| v as f64),
            as_f64(&b.file_descriptors, |v| v as f64),
            descending,
        ),
        ProcessColumn::StartTime => numeric(
            as_f64(&a.start_time, |v| v),
            as_f64(&b.start_time, |v| v),
            descending,
        ),
        ProcessColumn::Nice => numeric(
            as_f64(&a.nice, |v| v as f64),
            as_f64(&b.nice, |v| v as f64),
            descending,
        ),
        ProcessColumn::Cgroup => textual(&a.cgroup, &b.cgroup, descending),
        ProcessColumn::CommandLine => textual(&a.command_line, &b.command_line, descending),
    }
}

fn order_optional<T>(
    left: Option<T>,
    right: Option<T>,
    descending: bool,
    compare: impl Fn(&T, &T) -> Ordering,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            let ordering = compare(&left, &right);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::UnknownReason;

    fn process(pid: u32, name: &str, parent: u32, cpu: f64, memory: u64) -> ProcessFacts {
        let mut facts = ProcessFacts::synthetic(pid, name);
        facts.parent_pid = Field::Value(parent as u64);
        facts.cpu_utilization = Field::Value(cpu);
        facts.memory_resident = Field::Value(memory);
        facts
    }

    fn names(model: &ProcessTableModel) -> Vec<String> {
        model
            .visible_rows()
            .iter()
            .map(|row| model.processes[row.index].display_name())
            .collect()
    }

    fn sample() -> Vec<ProcessFacts> {
        vec![
            process(10, "systemd", 1, 0.01, 8_000_000),
            process(20, "firefox", 10, 0.75, 900_000_000),
            process(30, "gedit", 10, 0.05, 40_000_000),
            process(40, "firefox-tab", 20, 0.20, 300_000_000),
        ]
    }

    #[test]
    fn the_default_sort_puts_the_busiest_process_first() {
        let model = ProcessTableModel::new(sample());
        assert_eq!(model.sort().0, ProcessColumn::CpuUtilization);
        assert_eq!(names(&model)[0], "firefox");
        assert_eq!(model.visible_rows().len(), 4);
    }

    #[test]
    fn clicking_the_active_column_flips_the_direction() {
        let mut model = ProcessTableModel::new(sample());
        model.sort_by(ProcessColumn::Memory);
        assert_eq!(
            model.sort(),
            (ProcessColumn::Memory, SortDirection::Descending)
        );
        assert_eq!(names(&model)[0], "firefox");
        model.sort_by(ProcessColumn::Memory);
        assert_eq!(model.sort().1, SortDirection::Ascending);
        assert_eq!(names(&model)[0], "systemd");
    }

    #[test]
    fn a_text_column_starts_ascending_and_a_number_column_starts_descending() {
        let mut model = ProcessTableModel::new(sample());
        model.sort_by(ProcessColumn::Name);
        assert_eq!(model.sort().1, SortDirection::Ascending);
        assert_eq!(names(&model)[0], "firefox");
        model.sort_by(ProcessColumn::Threads);
        assert_eq!(model.sort().1, SortDirection::Descending);
    }

    #[test]
    fn a_process_with_no_reading_sorts_last_in_both_directions() {
        let mut processes = sample();
        processes.push({
            let mut unknown = process(50, "just-started", 10, 0.0, 0);
            unknown.cpu_utilization = Field::Unknown(UnknownReason::NotYetSampled);
            unknown
        });
        let mut model = ProcessTableModel::new(processes);

        assert_eq!(names(&model).last().unwrap(), "just-started");
        model.set_sort(ProcessColumn::CpuUtilization, SortDirection::Ascending);
        assert_eq!(
            names(&model).last().unwrap(),
            "just-started",
            "a missing reading is not the smallest value either"
        );
        // A measured zero still takes its place on the scale.
        assert_eq!(names(&model)[0], "systemd");
    }

    #[test]
    fn ties_are_broken_by_pid_so_a_refresh_does_not_shuffle_rows() {
        let processes = vec![
            process(300, "same", 1, 0.5, 100),
            process(100, "same", 1, 0.5, 100),
            process(200, "same", 1, 0.5, 100),
        ];
        let model = ProcessTableModel::new(processes.clone());
        let order: Vec<u32> = model
            .visible_rows()
            .iter()
            .map(|row| model.processes[row.index].pid)
            .collect();
        assert_eq!(order, vec![100, 200, 300]);

        let mut reordered = processes;
        reordered.reverse();
        let second = ProcessTableModel::new(reordered);
        let second_order: Vec<u32> = second
            .visible_rows()
            .iter()
            .map(|row| second.processes[row.index].pid)
            .collect();
        assert_eq!(order, second_order);
    }

    #[test]
    fn the_filter_matches_name_pid_and_user() {
        let mut model = ProcessTableModel::new(sample());
        model.set_filter("fire");
        assert_eq!(names(&model), vec!["firefox", "firefox-tab"]);
        model.set_filter("30");
        assert_eq!(names(&model), vec!["gedit"]);
        model.set_filter("tim");
        assert_eq!(model.visible_rows().len(), 4);
        model.set_filter("   ");
        assert_eq!(model.visible_rows().len(), 4);
    }

    #[test]
    fn a_hidden_command_line_is_not_searchable() {
        let mut processes = sample();
        processes[1].command_line = Field::Value("/usr/bin/firefox --secret-token abcdef".into());
        let mut model = ProcessTableModel::new(processes);
        model.set_filter("secret-token");
        assert!(
            model.visible_rows().is_empty(),
            "a filter must not confirm the contents of a hidden column"
        );

        model.set_show_command_line(true);
        assert_eq!(names(&model), vec!["firefox"]);
    }

    #[test]
    fn the_private_column_appears_only_when_it_is_turned_on() {
        let mut model = ProcessTableModel::new(sample());
        assert!(!model.columns().contains(&ProcessColumn::CommandLine));
        assert_eq!(model.columns().len(), ProcessColumn::ALL.len() - 1);
        model.set_show_command_line(true);
        assert!(model.columns().contains(&ProcessColumn::CommandLine));
    }

    #[test]
    fn tree_mode_nests_children_under_the_parent_that_is_in_view() {
        let mut model = ProcessTableModel::new(sample());
        model.set_tree_mode(true);
        let rows = model.visible_rows().to_vec();
        assert_eq!(rows.len(), 4);
        // systemd is the only root: pid 1 is not in the list.
        assert_eq!(model.processes[rows[0].index].display_name(), "systemd");
        assert_eq!(rows[0].depth, 0);
        assert!(rows[0].has_children);
        // Its children come next, busiest first, and firefox's own child is
        // one level deeper again.
        assert_eq!(model.processes[rows[1].index].display_name(), "firefox");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(model.processes[rows[2].index].display_name(), "firefox-tab");
        assert_eq!(rows[2].depth, 2);
        assert_eq!(model.processes[rows[3].index].display_name(), "gedit");
        assert_eq!(rows[3].depth, 1);
    }

    #[test]
    fn a_filtered_tree_keeps_the_ancestors_of_a_match() {
        let mut model = ProcessTableModel::new(sample());
        model.set_tree_mode(true);
        model.set_filter("firefox-tab");
        let shown = names(&model);
        assert_eq!(shown, vec!["systemd", "firefox", "firefox-tab"]);
        // In flat mode only the match itself is shown.
        model.set_tree_mode(false);
        assert_eq!(names(&model), vec!["firefox-tab"]);
    }

    #[test]
    fn a_parent_cycle_still_renders_every_row_exactly_once() {
        let mut a = process(10, "a", 20, 0.1, 1);
        let b = process(20, "b", 10, 0.2, 1);
        a.parent_pid = Field::Value(20);
        let mut model = ProcessTableModel::new(vec![a, b]);
        model.set_tree_mode(true);
        assert_eq!(model.visible_rows().len(), 2);
        let mut seen: Vec<u32> = model
            .visible_rows()
            .iter()
            .map(|row| model.processes[row.index].pid)
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![10, 20]);
    }

    #[test]
    fn a_process_that_claims_itself_as_its_parent_is_a_root() {
        let mut self_parented = process(10, "odd", 10, 0.1, 1);
        self_parented.parent_pid = Field::Value(10);
        let mut model = ProcessTableModel::new(vec![self_parented]);
        model.set_tree_mode(true);
        assert_eq!(model.visible_rows().len(), 1);
        assert_eq!(model.visible_rows()[0].depth, 0);
    }

    #[test]
    fn a_new_round_keeps_the_sort_and_the_filter() {
        let mut model = ProcessTableModel::new(sample());
        model.set_sort(ProcessColumn::Memory, SortDirection::Ascending);
        model.set_filter("fire");
        model.update(sample());
        assert_eq!(
            model.sort(),
            (ProcessColumn::Memory, SortDirection::Ascending)
        );
        assert_eq!(names(&model), vec!["firefox-tab", "firefox"]);
    }

    #[test]
    fn an_empty_round_produces_an_empty_table_rather_than_a_stale_one() {
        let mut model = ProcessTableModel::new(sample());
        model.update(Vec::new());
        assert!(model.visible_rows().is_empty());
        assert!(model.process_at(0).is_none());
    }
}
