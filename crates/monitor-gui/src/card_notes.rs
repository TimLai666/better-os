//! A note that explains one row of a card says which row it explains
//! (ticket 145). The owner could not tell what 「平均有多少工作正在執行或等著
//! 用處理器」 at the foot of the processor's card was about: it explained one
//! row of four and named none.
//!
//! Two shapes, by where the row is:
//!
//! - A row of a card's name and value list carries its note, and the note is
//!   drawn directly under that row as secondary text ([`Fact::note`]).
//! - A figure among the page's figures cannot hold a sentence under it, so
//!   its name and the note start with the same [`MARK`].
//!
//! A note about a whole card or page (the battery's estimate, the startup
//! list) belongs to no row and stays where it is.

use crate::charts::{CpuReadings, GpuReadings};
use crate::i18n::Copy;

/// The marker a figure's name and its note both carry.
pub(crate) const MARK: &str = "*";

/// One row of a card's name and value list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fact {
    pub(crate) name: &'static str,
    pub(crate) value: String,
    /// The value says why there is none, and is drawn as secondary text.
    pub(crate) missing: bool,
    /// What this row means, drawn directly under it.
    pub(crate) note: Option<&'static str>,
}

/// The processor card's rows, the load figure with its note.
pub(crate) fn cpu_facts(cpu: &CpuReadings, c: &'static Copy) -> Vec<Fact> {
    let fact = |name, value: Option<String>, missing_text: &str| Fact {
        name,
        missing: value.is_none(),
        value: value.unwrap_or_else(|| missing_text.to_string()),
        note: None,
    };
    vec![
        fact(
            c.maximum_speed,
            cpu.maximum_hz.map(crate::pages::hertz),
            c.not_available,
        ),
        fact(
            c.logical_processors,
            cpu.logical.map(|n| n.to_string()),
            c.not_available,
        ),
        Fact {
            note: Some(c.waiting_to_run_hint),
            ..fact(
                c.waiting_to_run,
                cpu.load_1m.map(|l| format!("{l:.2}")),
                c.not_available,
            )
        },
        fact(
            c.temperature,
            cpu.temperature_celsius.map(|t| format!("{t:.0} °C")),
            c.not_read,
        ),
    ]
}

/// The graphics card's utilization figure's name, marked when the figure
/// has the note [`gpu_utilization_note`] gives.
pub(crate) fn gpu_utilization_title(gpu: &GpuReadings, c: &'static Copy) -> String {
    match gpu_utilization_note(gpu, c) {
        Some(_) => format!("{}{MARK}", c.utilization),
        None => c.utilization.to_string(),
    }
}

/// What the utilization figure counts, when it is added up from programs'
/// own use rather than read from the driver.
pub(crate) fn gpu_utilization_note(gpu: &GpuReadings, c: &'static Copy) -> Option<String> {
    (gpu.source.as_deref() == Some("clients")).then(|| format!("{MARK}{}", c.busy_from_clients))
}
