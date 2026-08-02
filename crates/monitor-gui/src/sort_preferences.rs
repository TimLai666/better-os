use std::{env, fs, io, path::PathBuf};

const APP_COLUMN_KEY: &str = "apps-sort-column";
const APP_DESCENDING_KEY: &str = "apps-sort-descending";
const PROCESS_COLUMN_KEY: &str = "processes-sort-column";
const PROCESS_DESCENDING_KEY: &str = "processes-sort-descending";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SortPreference {
    pub column: String,
    pub descending: bool,
}

#[derive(Clone, Debug)]
struct SortPreferences {
    app: SortPreference,
    process: SortPreference,
}

impl Default for SortPreferences {
    fn default() -> Self {
        Self {
            app: SortPreference {
                column: "cpu".to_string(),
                descending: true,
            },
            process: SortPreference {
                column: "cpu".to_string(),
                descending: true,
            },
        }
    }
}

pub fn load_app() -> SortPreference {
    load().app
}

pub fn load_process() -> SortPreference {
    load().process
}

pub fn save_app(column: &str, descending: bool) -> io::Result<()> {
    let mut preferences = load();
    preferences.app = SortPreference {
        column: column.to_string(),
        descending,
    };
    preferences.save()
}

pub fn save_process(column: &str, descending: bool) -> io::Result<()> {
    let mut preferences = load();
    preferences.process = SortPreference {
        column: column.to_string(),
        descending,
    };
    preferences.save()
}

fn load() -> SortPreferences {
    let mut preferences = SortPreferences::default();
    let Ok(content) = fs::read_to_string(config_path()) else {
        return preferences;
    };

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            APP_COLUMN_KEY => preferences.app.column = value.trim().to_string(),
            APP_DESCENDING_KEY => preferences.app.descending = parse_bool(value),
            PROCESS_COLUMN_KEY => preferences.process.column = value.trim().to_string(),
            PROCESS_DESCENDING_KEY => preferences.process.descending = parse_bool(value),
            _ => {}
        }
    }
    preferences
}

impl SortPreferences {
    fn save(&self) -> io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = format!(
            "# Better Monitor table sorting\n{APP_COLUMN_KEY}={}\n{APP_DESCENDING_KEY}={}\n{PROCESS_COLUMN_KEY}={}\n{PROCESS_DESCENDING_KEY}={}\n",
            self.app.column, self.app.descending, self.process.column, self.process.descending,
        );
        let temporary = path.with_extension("conf.tmp");
        fs::write(&temporary, content)?;
        fs::rename(temporary, path)
    }
}

fn parse_bool(value: &str) -> bool {
    matches!(value.trim(), "1" | "true" | "yes" | "on")
}

fn config_path() -> PathBuf {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("better-os")
            .join("monitor-table-sort.conf");
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("better-os")
        .join("monitor-table-sort.conf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_parser_accepts_monitor_config_values() {
        for value in ["1", "true", "yes", "on"] {
            assert!(parse_bool(value));
        }
        for value in ["0", "false", "no", "off", ""] {
            assert!(!parse_bool(value));
        }
    }
}
