use orbit_api::{orbit_config, ui::sctk::Anchor};

#[orbit_config]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub max_results: usize,
    pub icon_size: u32,
    /// "top", "center", or "bottom"
    pub position: String,
    pub launch_options: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 600,
            height: 420,
            max_results: 8,
            icon_size: 32,
            position: "center".to_owned(),
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
