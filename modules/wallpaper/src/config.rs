use std::path::PathBuf;

use serde::{Deserialize, Serialize};

mod serde_pct {
    use serde::{Deserialize, Deserializer, Serializer, de};

    const SCALE: f32 = 1000.0;

    pub fn serialize<S>(v: &f32, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let rounded = (v * SCALE).round() / SCALE;
        s.serialize_f64(rounded as f64)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<f32, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum NumOrStr {
            N(f32),
            S(String),
        }

        match NumOrStr::deserialize(d)? {
            NumOrStr::N(n) => Ok(n),
            NumOrStr::S(s) => s.parse::<f32>().map_err(de::Error::custom),
        }
    }
}

fn default_clock_font_size() -> f32 {
    48.0
}
fn default_time_format() -> String {
    "%H:%M".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum WidgetConfig {
    Clock {
        #[serde(with = "serde_pct")]
        x: f32,
        #[serde(with = "serde_pct")]
        y: f32,
        #[serde(default = "default_clock_font_size")]
        font_size: f32,
        #[serde(default = "default_time_format")]
        time_format: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
