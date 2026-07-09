use std::{borrow::Cow, path::PathBuf, time::Duration};

use chrono::{DateTime, Local};
use orbit_api::{
    orbit_config,
    ui::{
        model::{Family, Size},
        widget::{Column, Element, Length, Overlay, Row, Text},
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
    pub widgets: Vec<Placed>,
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

#[orbit_config]
pub struct Placed {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(flatten)]
    pub widget: WidgetConfig,
}

impl Placed {
    pub fn place(&self, target: &PerTarget, now: &DateTime<Local>, on: &mut Overlay<Msg>) {
        on.push(
            self.widget.element(now),
            (target.size.width as f32 * self.x.clamp(0.0, 1.0)).ceil() as i32,
            (target.size.height as f32 * self.y.clamp(0.0, 1.0)).ceil() as i32,
        );
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
        #[serde(default = "default_clock_font_size")]
        font_size: f32,
        #[serde(default)]
        font_family: Option<FontFamilyConfig>,
        #[serde(default = "default_time_format")]
        time_format: String,
    },
    Column {
        #[serde(default)]
        spacing: i32,
        children: Vec<WidgetConfig>,
    },
    Row {
        #[serde(default)]
        spacing: i32,
        children: Vec<WidgetConfig>,
    },
}

impl WidgetConfig {
    pub fn contains_clock(&self) -> bool {
        match self {
            Self::Clock { .. } => true,
            Self::Column { children, .. } | Self::Row { children, .. } => {
                children.iter().any(Self::contains_clock)
            }
        }
    }

    pub fn clock_durations(&self, out: &mut Vec<Duration>) {
        match self {
            Self::Clock { time_format: f, .. } => out.push(if f.contains("%f") {
                Duration::from_millis(100)
            } else if f.contains("%S") {
                Duration::from_secs(1)
            } else if f.contains("%M") {
                Duration::from_secs(60)
            } else {
                Duration::from_secs(3600)
            }),
            Self::Column { children, .. } | Self::Row { children, .. } => {
                for c in children {
                    c.clock_durations(out);
                }
            }
        }
    }

    fn element(&self, now: &DateTime<Local>) -> Element<Msg> {
        match self {
            WidgetConfig::Clock {
                font_size,
                font_family,
                time_format,
            } => {
                let time = now.format(time_format).to_string();
                let mut text = Text::new(time)
                    .wrap(orbit_api::ui::model::Wrap::None)
                    .font_size(*font_size)
                    .family(Family::Monospace)
                    .size(Size::splat(Length::Fit));
                if let Some(family) = font_family {
                    text = text.family(family.clone().into());
                }
                text.into()
            }
            WidgetConfig::Column { spacing, children } => {
                let mut col = Column::empty().spacing(*spacing);
                for child in children {
                    col.push(child.element(now));
                }
                col.into()
            }
            WidgetConfig::Row { spacing, children } => {
                let mut row = Row::empty().spacing(*spacing);
                for child in children {
                    row.push(child.element(now));
                }
                row.into()
            }
        }
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
