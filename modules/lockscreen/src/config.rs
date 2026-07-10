use orbit_api::orbit_config;

fn default_message() -> String {
    "Welcome {username}!".into()
}
fn default_idle_duration() -> String {
    "5m".into()
}
fn default_blur() -> u32 {
    8
}
fn default_bg_tint() -> u8 {
    0x5e
}

#[orbit_config]
pub struct Config {
    #[serde(default = "default_message")]
    pub message: String,
    #[serde(default = "default_idle_duration")]
    pub idle: String,
    #[serde(default = "default_blur")]
    pub blur: u32,
    #[serde(default = "default_bg_tint")]
    pub bg_tint: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            message: default_message(),
            idle: default_idle_duration(),
            blur: default_blur(),
            bg_tint: default_bg_tint(),
        }
    }
}
