from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one marker, found {count}")
    return text.replace(old, new, 1)


app = Path("crates/monitor-gui/src/app.rs")
text = app.read_text()

if "INACTIVE_SURFACE_REFRESH_INTERVAL" not in text:
    text = replace_once(
        text,
        "const MIB: f64 = 1024.0 * 1024.0;\nconst SECTOR_SIZE: u64 = 512;\n",
        """const MIB: f64 = 1024.0 * 1024.0;
const SECTOR_SIZE: u64 = 512;
const INACTIVE_SURFACE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

fn surface_refresh_due(window_active: bool, elapsed_since_refresh: Duration) -> bool {
    window_active || elapsed_since_refresh >= INACTIVE_SURFACE_REFRESH_INTERVAL
}
""",
        "refresh policy",
    )
    text = replace_once(
        text,
        "    previous_block_counters: HashMap<String, BlockCounters>,\n    last_disk_sample: Instant,\n",
        "    previous_block_counters: HashMap<String, BlockCounters>,\n    last_disk_sample: Instant,\n    last_surface_refresh: Instant,\n",
        "monitor fields",
    )
    text = replace_once(
        text,
        """            last_disk_sample: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        };

        monitor.collect_metrics(cx);
        cx.spawn(async move |this, cx| {
""",
        """            last_disk_sample: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            last_surface_refresh: Instant::now(),
        };

        let window_handle = window.window_handle();
        monitor.collect_metrics(true, cx);
        cx.spawn(async move |this, cx| {
""",
        "timer setup",
    )
    text = replace_once(
        text,
        """                Timer::after(delay).await;
                if this
                    .update(cx, |this, cx| {
                        this.collect_metrics(cx);
                        cx.notify();
                    })
                    .is_err()
                {
""",
        """                Timer::after(delay).await;
                let window_active = window_handle
                    .update(cx, |_, window, _| window.is_window_active())
                    .unwrap_or(true);
                if this
                    .update(cx, |this, cx| {
                        let refresh_surfaces =
                            this.should_refresh_surfaces(window_active, Instant::now());
                        this.collect_metrics(refresh_surfaces, cx);
                        if refresh_surfaces {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
""",
        "timer refresh",
    )
    text = replace_once(
        text,
        "    fn collect_metrics(&mut self, cx: &mut Context<Self>) {\n",
        """    fn should_refresh_surfaces(&mut self, window_active: bool, now: Instant) -> bool {
        let should_refresh = surface_refresh_due(
            window_active,
            now.saturating_duration_since(self.last_surface_refresh),
        );
        if should_refresh {
            self.last_surface_refresh = now;
        }
        should_refresh
    }

    fn collect_metrics(&mut self, refresh_surfaces: bool, cx: &mut Context<Self>) {
""",
        "collector signature",
    )
    if text.count("if !table_refresh_held {") != 2:
        raise SystemExit("table refresh guards: expected two markers")
    text = text.replace(
        "if !table_refresh_held {",
        "if refresh_surfaces && !table_refresh_held {",
    )

if "inactive_windows_coalesce_surface_refreshes" not in text:
    text = text.rstrip() + """

#[cfg(test)]
mod visibility_tests {
    use super::*;

    #[test]
    fn active_windows_refresh_every_sample() {
        assert!(surface_refresh_due(true, Duration::ZERO));
    }

    #[test]
    fn inactive_windows_coalesce_surface_refreshes() {
        assert!(!surface_refresh_due(
            false,
            INACTIVE_SURFACE_REFRESH_INTERVAL - Duration::from_millis(1),
        ));
        assert!(surface_refresh_due(
            false,
            INACTIVE_SURFACE_REFRESH_INTERVAL,
        ));
    }
}
"""

app.write_text(text)

checklist = Path("docs/better-monitor-resources-v1.10.2-parity.md")
text = checklist.read_text()
text = text.replace(
    "| Pause graphical updates when hidden | ⬜ | Manual graph pause exists; visibility-driven throttling is missing. |",
    "| Pause graphical updates when hidden | ✅ | Collection remains at the configured interval; an inactive window coalesces table/chart redraws to at most once every five seconds and resumes full-rate rendering when active. |",
)
text = text.replace(
    "| Collection independent from rendering | 🟨 | Manual graph pause preserves collection. Long-running service ownership remains outside this PR. |",
    "| Collection independent from rendering | 🟨 | Manual graph pause and inactive-window throttling preserve collection. Long-running service ownership remains outside this PR. |",
)
checklist.write_text(text)

Path("docs/better-monitor-visibility-throttling.md").write_text("""# Better Monitor visibility throttling

Better Monitor keeps collecting at the configured 250 ms to 3 s interval even
when its window is not active. Collection updates the bounded histories,
incident evidence, device maxima, and in-memory process/application models.

Presentation work is separate. While the window is active, tables and charts
refresh at the configured collection interval. While inactive or minimized,
Better Monitor coalesces table/chart refresh and window notifications to at
most once every five seconds. This avoids spending rendering work on a window
the user is not looking at without creating gaps in the collected evidence.

The next scheduled sample restores full-rate presentation after the window
becomes active. The policy is covered by unit tests and does not change the
manual chart-pause behavior.
""")
