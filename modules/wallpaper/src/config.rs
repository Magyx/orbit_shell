use std::{borrow::Cow, path::PathBuf, time::Duration};

use orbit_api::{
    orbit_config,
    ui::{
        model::{Family, Size},
        widget::{Element, Length, Overlay, Rectangle, Text},
    },
};

use crate::{Msg, PerTarget};

fn default_source() -> PathBuf {
    xdg_user::pictures()
        .unwrap_or_default()
        .unwrap_or_default()
        .join("Wallpapers")
}
fn default_cycle() -> String {
    "1h".into()
}

#[orbit_config]
pub struct Config {
    #[serde(default = "default_source")]
    pub source: PathBuf,
    #[serde(default = "default_cycle")]
    pub cycle: String,
    #[serde(default)]
    pub widgets: Vec<WidgetConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source: default_source(),
            cycle: default_cycle(),
            widgets: Vec::new(),
        }
    }
}

fn default_clock_font_size() -> f32 {
    48.0
}
fn default_time_format() -> String {
    "%H:%M".to_string()
}

#[orbit_config]
#[serde(tag = "type")]
pub enum WidgetConfig {
    Clock {
        x: f32,
        y: f32,
        #[serde(default = "default_clock_font_size")]
        font_size: f32,
        #[serde(default)]
        font_family: Option<FontFamilyConfig>,
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

    pub fn place(&self, target: &PerTarget, on: &mut Overlay<Msg>) {
        let (element, x, y): (Element<Msg>, f32, f32) = match self {
            WidgetConfig::Clock {
                x,
                y,
                font_size,
                font_family,
                time_format,
            } => {
                let time = chrono::Local::now().format(time_format).to_string();
                let mut text = Text::new(time, *font_size)
                    .family(Family::Monospace)
                    .size(Size::splat(Length::Fit));
                if let Some(family) = font_family {
                    text = text.family(family.clone().into());
                }
                (text.into(), *x, *y)
            }
            WidgetConfig::None => (Rectangle::placeholder().into(), 0.0, 0.0),
        };

        on.push(
            element,
            (target.size.width as f32 * x.clamp(0.0, 1.0)).ceil() as i32,
            (target.size.height as f32 * y.clamp(0.0, 1.0)).ceil() as i32,
        );
    }
}

#[orbit_config]
pub enum FontFamilyConfig {
    Monospace,
    SansSerif,
    Serif,
    #[serde(untagged)]
    Name(String),
}

impl From<FontFamilyConfig> for Family {
    fn from(config: FontFamilyConfig) -> Self {
        match config {
            FontFamilyConfig::Monospace => Family::Monospace,
            FontFamilyConfig::SansSerif => Family::SansSerif,
            FontFamilyConfig::Serif => Family::Serif,
            FontFamilyConfig::Name(s) => Family::Name(Cow::Owned(s)),
        }
    }
}
