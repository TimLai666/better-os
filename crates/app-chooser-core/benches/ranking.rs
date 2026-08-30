//! MIME compatibility filtering latency against a synthetic catalog.
//!
//! The chooser's ranking is what runs between "the user pressed Open With" and
//! the first frame, so it is the number that matters. The catalog is synthetic
//! and 5,000 records deep, matching the shared catalog's own benchmark, because
//! whatever happens to be installed on the machine running this proves nothing
//! repeatable.
//!
//! No criterion dependency: the workspace measures wall time directly, the same
//! way `app-catalog-core`'s benchmarks do.

use std::path::PathBuf;
use std::time::Instant;

use app_catalog_core::{
    ApplicationRecord, DesktopEnvironments, DesktopFile, DesktopId, EntryScope, MimeType, NoProbe,
};
use app_chooser_core::{ChooserRequest, MimeAppsFile, MimeResolution, UsageHistory, ranking::rank};

const RECORDS: usize = 5_000;
const ITERATIONS: usize = 20;

fn mime(value: &str) -> MimeType {
    MimeType::parse(value).expect("valid mime type")
}

/// A catalog where a tenth of the records declare the selected type, a fifth
/// declare its parent, and the rest are unrelated. A catalog where everything
/// matched would measure sorting, not filtering.
fn synthetic_catalog() -> Vec<ApplicationRecord> {
    (0..RECORDS)
        .map(|index| {
            let mime_types = match index % 10 {
                0 => "text/x-rust;",
                1 | 2 => "text/plain;",
                3 => "text/*;",
                _ => "application/vnd.example.binary;",
            };
            let body = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=Application {index}\n\
                 Name[zh_TW]=應用程式 {index}\n\
                 Exec=app-{index} %U\n\
                 Categories=Utility;Development;\n\
                 MimeType={mime_types}\n"
            );
            let file = DesktopFile::parse(&body).expect("valid entry");
            ApplicationRecord::from_desktop_file(
                DesktopId::new(format!("app-{index}.desktop")).expect("valid id"),
                PathBuf::from(format!("/usr/share/applications/app-{index}.desktop")),
                EntryScope::System,
                &file,
                &NoProbe,
            )
            .expect("valid record")
        })
        .collect()
}

fn main() {
    let records = synthetic_catalog();
    let associations =
        MimeAppsFile::parse("[Default Applications]\ntext/x-rust=app-10.desktop\n").associations();
    let resolution = MimeResolution {
        requested: mime("text/x-rust"),
        primary: mime("text/x-rust"),
        ancestors: vec![mime("text/plain")],
    };
    let history = UsageHistory::from_associations(&associations, &resolution.primary);
    let environments = DesktopEnvironments::new(["GNOME"]);
    let request = ChooserRequest {
        resolution: &resolution,
        associations: &associations,
        history: &history,
        environments: &environments,
        locale: None,
    };

    // One warm pass so the measurement is not dominated by first-touch page
    // faults on the record vector.
    let sections = rank(&records, &request);
    println!(
        "sections: {} recommended, {} other, {} all",
        sections.recommended.len(),
        sections.other.len(),
        sections.all.len()
    );

    let started = Instant::now();
    for _ in 0..ITERATIONS {
        let sections = rank(&records, &request);
        std::hint::black_box(&sections);
    }
    let elapsed = started.elapsed();
    println!(
        "mime compatibility filtering over {RECORDS} records: {:.3} ms per pass ({ITERATIONS} passes)",
        elapsed.as_secs_f64() * 1000.0 / ITERATIONS as f64
    );
}
