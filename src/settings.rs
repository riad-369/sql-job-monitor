use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    #[serde(default)]
    pub theme_mode: i32,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default = "default_history_width")]
    pub history_width: f32,
    #[serde(default)]
    pub history_collapsed: bool,
    #[serde(default)]
    pub timeline_days: i32,
    #[serde(default = "default_timeline_range")]
    pub timeline_range: i32,
    #[serde(default = "default_timeline_pph")]
    pub timeline_pph: f32,
    #[serde(default)]
    pub timeline_zoom_saved: bool,
    #[serde(default = "default_columns")]
    pub columns: [f32; 9],
    #[serde(default)]
    pub last_connection_id: String,
    #[serde(default)]
    pub auto_refresh: bool,
    #[serde(default = "default_refresh_seconds")]
    pub refresh_seconds: i32,
}

fn default_history_width() -> f32 {
    365.0
}

fn default_timeline_range() -> i32 {
    1
}

fn default_timeline_pph() -> f32 {
    60.0
}

fn default_refresh_seconds() -> i32 {
    5
}

fn default_columns() -> [f32; 9] {
    [260.0, 130.0, 78.0, 280.0, 178.0, 84.0, 158.0, 140.0, 170.0]
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            theme_mode: 0,
            pinned: false,
            history_width: default_history_width(),
            history_collapsed: false,
            timeline_days: 0,
            timeline_range: default_timeline_range(),
            timeline_pph: default_timeline_pph(),
            timeline_zoom_saved: false,
            columns: default_columns(),
            last_connection_id: String::new(),
            auto_refresh: false,
            refresh_seconds: default_refresh_seconds(),
        }
    }
}

impl UiSettings {
    pub fn normalized(mut self) -> Self {
        self.theme_mode = self.theme_mode.clamp(0, 2);
        self.history_width = self.history_width.clamp(280.0, 1600.0);
        self.timeline_days = match self.timeline_days {
            1 | 3 | 7 => self.timeline_days,
            _ => 0,
        };
        self.timeline_range = match self.timeline_range {
            3 | 7 => self.timeline_range,
            _ => 1,
        };
        self.timeline_pph = self.timeline_pph.clamp(4.0, 6000.0);
        self.refresh_seconds = self.refresh_seconds.clamp(2, 300);
        for width in &mut self.columns {
            *width = width.clamp(72.0, 1200.0);
        }
        self
    }
}

pub fn settings_path() -> PathBuf {
    crate::paths::app_data_dir().join("settings.json")
}

pub fn load() -> UiSettings {
    let Ok(text) = fs::read_to_string(settings_path()) else {
        return UiSettings::default();
    };
    serde_json::from_str::<UiSettings>(&text)
        .unwrap_or_default()
        .normalized()
}

pub fn store(settings: &UiSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let stored = settings.clone().normalized();
    let text = serde_json::to_string_pretty(&stored).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}
