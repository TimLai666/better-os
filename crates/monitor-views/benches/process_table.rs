//! The large-process benchmark Issue #16 requires.
//!
//! What the table has to survive is not drawing ten thousand rows — the
//! virtualized table only ever draws the visible window — but rebuilding the
//! model behind them. Every sampling tick re-reads the whole process list,
//! re-sorts it, re-applies the filter, and regroups it into applications, and
//! all of that runs before a frame can be drawn. Those are the numbers here.
//!
//! There is no benchmark framework dependency, matching `app-catalog-core`:
//! these are wall-clock timings of exact operations, and a statistics harness
//! would not change any decision made from them.
//!
//! Run with `cargo bench -p monitor-views`, or
//! `cargo bench -p monitor-views -- --test` for a single-iteration smoke run.

use monitor_views::apps::AppsModel;
use monitor_views::facts::ProcessFacts;
use monitor_views::field::Field;
use monitor_views::grouping::{GroupingPrecedence, group_processes};
use monitor_views::process_table::{ProcessColumn, ProcessTableModel, SortDirection};
use std::time::{Duration, Instant};

/// A process tree shaped like a desktop under load: a few hundred
/// applications, each with a handful of helper processes, plus system
/// services and kernel threads that report nothing.
fn synthetic_processes(count: usize) -> Vec<ProcessFacts> {
    let mut processes = Vec::with_capacity(count);
    let mut pid = 2u32;
    let names = [
        "firefox",
        "chromium",
        "code",
        "gnome-shell",
        "nautilus",
        "gimp",
        "libreoffice",
        "steam",
        "thunderbird",
        "spotify",
    ];
    while processes.len() < count {
        let app = processes.len() / 8;
        let name = names[app % names.len()];
        let leader = pid;
        let scope = format!(
            "/user.slice/user-1000.slice/user@1000.service/app.slice/app-gnome-org.example.{name}{app}-{leader}.scope"
        );
        for member in 0..8 {
            if processes.len() >= count {
                break;
            }
            let mut process = ProcessFacts::synthetic(pid, name);
            process.parent_pid = Field::Value(if member == 0 { 1 } else { leader as u64 });
            process.cgroup = Field::Value(scope.clone());
            process.cpu_utilization = Field::Value((pid % 97) as f64 / 100.0);
            process.memory_resident = Field::Value((pid as u64 % 5_000) * 1_024 * 64);
            process.read_rate = Field::Value((pid % 31) as f64 * 1024.0);
            process.write_rate = Field::Value((pid % 17) as f64 * 1024.0);
            // Every twentieth process has an unreadable descriptor count, so
            // the sort has to handle missing values at scale rather than only
            // in a unit test.
            if pid % 20 == 0 {
                process.file_descriptors = Field::PermissionDenied {
                    path: format!("/proc/{pid}/fd"),
                };
            }
            processes.push(process);
            pid += 1;
        }
    }
    processes
}

fn measure(label: &str, iterations: usize, mut body: impl FnMut()) -> Duration {
    // One untimed pass so the first allocation is not counted as latency.
    body();
    let start = Instant::now();
    for _ in 0..iterations {
        body();
    }
    let elapsed = start.elapsed() / iterations as u32;
    println!("{label:<44} {:>9.3} ms", elapsed.as_secs_f64() * 1_000.0);
    elapsed
}

fn main() {
    let smoke = std::env::args().any(|argument| argument == "--test");
    let iterations = if smoke { 1 } else { 20 };
    let precedence = GroupingPrecedence::default();

    for count in [100usize, 1_000, 10_000] {
        println!("\n{count} processes");
        let processes = synthetic_processes(count);

        measure("build model + initial sort", iterations, || {
            let model = ProcessTableModel::new(processes.clone());
            std::hint::black_box(model.visible_rows().len());
        });

        let mut model = ProcessTableModel::new(processes.clone());
        measure("re-sort by memory", iterations, || {
            model.set_sort(ProcessColumn::Memory, SortDirection::Descending);
            std::hint::black_box(model.visible_rows().len());
        });
        measure("apply filter", iterations, || {
            model.set_filter("firefox");
            std::hint::black_box(model.visible_rows().len());
        });
        model.set_filter("");
        measure("build tree view", iterations, || {
            model.set_tree_mode(true);
            model.set_tree_mode(false);
        });
        measure("update with a new round", iterations, || {
            model.update(processes.clone());
            std::hint::black_box(model.visible_rows().len());
        });
        measure("group into applications", iterations, || {
            let grouping = group_processes(&processes, &precedence);
            std::hint::black_box(grouping.len());
        });
        measure("apps model with aggregates", iterations, || {
            let apps = AppsModel::new(processes.clone(), precedence.clone());
            std::hint::black_box(apps.applications().len());
        });
    }
}
