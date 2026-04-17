use orbit_api::{orbit_config, ui::sctk::Anchor};

fn default_width() -> u32 {
    600
}
fn default_height() -> u32 {
    420
}
fn default_max_results() -> usize {
    8
}
fn default_icon_size() -> u32 {
    32
}
fn default_position() -> String {
    "center".to_owned()
}

#[orbit_config]
pub struct Config {
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    #[serde(default = "default_icon_size")]
    pub icon_size: u32,
    #[serde(default = "default_position")]
    pub position: String,
    #[serde(default)]
    pub launch_options: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: default_width(),
            height: default_height(),
            max_results: default_max_results(),
            icon_size: default_icon_size(),
            position: default_position(),
            launch_options: "".to_owned(),
        }
    }
}

pub fn anchors_for_position(position: &str) -> Anchor {
    match position {
        "top" => Anchor::TOP,
        "bottom" => Anchor::BOTTOM,
        _ => Anchor::empty(), // center — compositor places it in the middle
    }
}
