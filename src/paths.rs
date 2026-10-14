use std::{ffi::OsString, path::PathBuf};

pub const APP_NAME: &str = "SQL Job Monitor";

/// Settings directory for this process.
///
/// Windows keeps `%APPDATA%\SQL Job Monitor`. macOS uses
/// `~/Library/Application Support/SQL Job Monitor`. `SQL_JOB_MONITOR_DATA_DIR`
/// overrides either one so tests never touch a real profile.
pub fn app_data_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        windows_app_data_dir(
            std::env::var_os("SQL_JOB_MONITOR_DATA_DIR"),
            std::env::var_os("APPDATA"),
        )
    } else {
        macos_app_data_dir(
            std::env::var_os("SQL_JOB_MONITOR_DATA_DIR"),
            std::env::var_os("HOME"),
        )
    }
}

/// `%APPDATA%\SQL Job Monitor`, or `.` when `APPDATA` is unset.
pub fn windows_app_data_dir(override_dir: Option<OsString>, appdata: Option<OsString>) -> PathBuf {
    if let Some(dir) = override_dir.filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    appdata
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_NAME)
}

/// `~/Library/Application Support/SQL Job Monitor`.
pub fn macos_app_data_dir(override_dir: Option<OsString>, home: Option<OsString>) -> PathBuf {
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
    fn windows_uses_appdata() {
        assert_eq!(
            windows_app_data_dir(None, Some(r"C:\Users\ada\AppData\Roaming".into())),
            PathBuf::from(r"C:\Users\ada\AppData\Roaming").join(APP_NAME)
        );
        assert_eq!(
            windows_app_data_dir(None, None),
            PathBuf::from(".").join(APP_NAME)
        );
        assert_eq!(
            windows_app_data_dir(Some(r"D:\profile".into()), Some("unused".into())),
            PathBuf::from(r"D:\profile")
        );
    }

    #[test]
    fn macos_uses_application_support_under_home() {
        assert_eq!(
            macos_app_data_dir(None, Some("/Users/ada".into())),
            PathBuf::from("/Users/ada/Library/Application Support/SQL Job Monitor")
        );
        assert_eq!(
            macos_app_data_dir(Some("/tmp/profile".into()), Some("/Users/ada".into())),
            PathBuf::from("/tmp/profile")
        );
        assert_eq!(
            macos_app_data_dir(Some("".into()), Some("/Users/ada".into())),
            PathBuf::from("/Users/ada/Library/Application Support/SQL Job Monitor")
        );
        assert_eq!(
            macos_app_data_dir(None, None),
            std::env::temp_dir().join(APP_NAME)
        );
    }
}
