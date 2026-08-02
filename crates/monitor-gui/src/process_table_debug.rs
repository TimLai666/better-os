use std::fmt;

use crate::process_table::ProcessTableDelegate;

impl fmt::Debug for ProcessTableDelegate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessTableDelegate")
            .field("visible_processes", &self.processes.len())
            .field("selected_processes", &self.selected_count())
            .finish_non_exhaustive()
    }
}
