use orbit_api::{serde, ui::sctk::Anchor};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(crate = "orbit_api::serde")]
pub struct Config {
    pub width: u32,
    pub height: u32,
    pub max_results: usize,
    pub icon_size: u32,
    /// "top", "center", or "bottom"
    pub position: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 600,
            height: 420,
            max_results: 8,
            icon_size: 32,
            position: "center".to_owned(),
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
