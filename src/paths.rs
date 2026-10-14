use std::{ffi::OsString, path::PathBuf};

pub const APP_NAME: &str = "SQL Job Monitor";

/// `~/Library/Application Support/SQL Job Monitor`. `SQL_JOB_MONITOR_DATA_DIR`
/// overrides it so tests and side-by-side profiles never touch real settings.
pub fn app_data_dir() -> PathBuf {
    app_data_dir_from(
        std::env::var_os("SQL_JOB_MONITOR_DATA_DIR"),
        std::env::var_os("HOME"),
    )
}

pub fn app_data_dir_from(override_dir: Option<OsString>, home: Option<OsString>) -> PathBuf {
    if let Some(dir) = override_dir.filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    match home.filter(|home| !home.is_empty()) {
        Some(home) => PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join(APP_NAME),
        None => std::env::temp_dir().join(APP_NAME),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_application_support_under_home() {
        assert_eq!(
            app_data_dir_from(None, Some("/Users/ada".into())),
            PathBuf::from("/Users/ada/Library/Application Support/SQL Job Monitor")
        );
    }

    #[test]
    fn override_wins_and_empty_values_are_ignored() {
        assert_eq!(
            app_data_dir_from(Some("/tmp/profile".into()), Some("/Users/ada".into())),
            PathBuf::from("/tmp/profile")
        );
        assert_eq!(
            app_data_dir_from(Some("".into()), Some("/Users/ada".into())),
            PathBuf::from("/Users/ada/Library/Application Support/SQL Job Monitor")
        );
        assert_eq!(
            app_data_dir_from(None, None),
            std::env::temp_dir().join(APP_NAME)
        );
    }
}
