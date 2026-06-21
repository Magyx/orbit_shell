use orbit_api::{Key, ui::render::texture::TextureHandle};

/// The wallpaper texture for an output. Produced by `wallpaper`.
pub const WALLPAPER_TEX: Key<TextureHandle> = Key::per_output("wallpaper/tex");
