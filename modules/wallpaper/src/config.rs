use std::path::PathBuf;

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
