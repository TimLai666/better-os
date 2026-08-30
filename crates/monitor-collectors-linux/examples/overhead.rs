//! Measure what one sampling round of every Linux collector costs.
//!
//! Run with `cargo run -p monitor-collectors-linux --release --example
//! overhead -- [rounds]`. The default is 50 rounds against the real `/proc`
//! and `/sys`.

use monitor_collectors_linux::{ProcessPrivacy, Roots, measure};

fn main() {
    let rounds = std::env::args()
        .nth(1)
        .and_then(|argument| argument.parse::<u32>().ok())
        .unwrap_or(50);

    let report = measure(&Roots::system(), ProcessPrivacy::default(), rounds);

    println!("rounds: {}", report.rounds);
    println!("total wall:  {:?}", report.total_wall);
    println!("mean round:  {:?}", report.mean_wall);
    println!("worst round: {:?}", report.worst_wall);
    match report.cpu_time {
        Some(cpu) => println!(
            "process cpu: {cpu:?} ({:.1}% of wall)",
            report.cpu_fraction().unwrap_or(0.0) * 100.0
        ),
        None => println!("process cpu: unavailable (/proc/self/stat unreadable)"),
    }
    println!();
    println!("{:<18} {:>12} {:>12}", "collector", "mean", "worst");
    for cost in &report.per_collector {
        println!(
            "{:<18} {:>12} {:>12}",
            cost.collector,
            format!("{:?}", cost.mean_wall),
            format!("{:?}", cost.worst_wall)
        );
    }
}
