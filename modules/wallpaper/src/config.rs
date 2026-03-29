use std::{path::PathBuf, time::Duration};

use orbit_api::serde::{Deserialize, Serialize};

fn default_clock_font_size() -> f32 {
    48.0
}
fn default_time_format() -> String {
    "%H:%M".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "orbit_api::serde", tag = "type", rename_all = "kebab-case")]
pub enum WidgetConfig {
    Clock {
        x: f32,
        y: f32,
        #[serde(default = "default_clock_font_size")]
        font_size: f32,
        #[serde(default = "default_time_format")]
        time_format: String,
    },
    None,
}

impl WidgetConfig {
    pub fn clock_duration(&self) -> Option<Duration> {
        match self {
            Self::Clock { time_format, .. } => Some(match time_format {
                f if f.contains("%f") => Duration::from_millis(100),
                f if f.contains("%S") => Duration::from_secs(1),
                f if f.contains("%M") => Duration::from_secs(60),
                _ => Duration::from_secs(3600),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(crate = "orbit_api::serde")]
pub struct Config {
    pub source: PathBuf,
    pub cycle: String,
    pub widgets: Vec<WidgetConfig>,
}

impl Default for Config {
    fn default() -> Self {
        let home = xdg_user::pictures().unwrap_or_default().unwrap_or_default();
        Self {
            source: home.join("Wallpapers"),
            cycle: "1h".into(),
            widgets: Vec::new(),
        }
    }
}
