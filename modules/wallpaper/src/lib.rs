use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use orbit_api::{
    ErasedMsg, Event, OrbitLoop, OrbitModule, orbit_plugin,
    ui::{
        graphics::{Engine, TargetId},
        model::Size,
        render::texture::TextureHandle,
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{Container, Element, Image, Length, Text, Widget},
    },
};
use serde::{Deserialize, Serialize};

mod helpers;

#[derive(Clone, Debug)]
pub enum Msg {
    None,
}

fn default_source() -> PathBuf {
    let cfg_path = helpers::xdg_home().canonicalize().unwrap();
    cfg_path.join("Pictures/Wallpapers")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default = "default_source")]
    pub source: PathBuf,
}

#[derive(Default)]
pub struct Wallpaper {
    textures: HashMap<TargetId, TextureHandle>,
    cfg: Config,
}

impl Wallpaper {
    fn pick_image(path: &Path) -> Option<PathBuf> {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        let Ok(rd) = std::fs::read_dir(path) else {
            return None;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            if let Some(ext) = p
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.to_ascii_lowercase())
                && (ext == "jpg" || ext == "jpeg" || ext == "png")
            {
                return Some(p);
            }
        }
        None
    }

    fn ensure_texture_loaded<'a>(
        &mut self,
        tid: &TargetId,
        engine: &mut Engine<'a, ErasedMsg>,
    ) -> bool {
        if self.textures.contains_key(tid) || !self.cfg.source.exists() {
            return false;
        }
        if let Some(p) = Self::pick_image(Path::new(&self.cfg.source))
            && let Ok(reader) = image::ImageReader::open(&p)
            && let Ok(img) = reader.decode()
        {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let handle = engine.load_texture_rgba8(w, h, rgba.as_raw());
            self.textures.insert(*tid, handle);
            return true;
        }
        false
    }
}

impl OrbitModule for Wallpaper {
    type Config = Config;
    type Message = Msg;

    fn cleanup<'a>(&mut self, engine: &mut Engine<'a, ErasedMsg>) {
        for (_, handle) in self.textures.drain() {
            engine.unload_texture(handle);
        }
    }

    fn config_updated<'a>(&mut self, engine: &mut Engine<'a, ErasedMsg>, cfg: &serde_yml::Value) {
        if let Ok(parsed) = serde_yml::from_value::<Config>(cfg.clone()) {
            if self.cfg.source != parsed.source {
                Self::cleanup(self, engine);
            }
            self.cfg = parsed;
        }
    }

    fn update<'a>(
        &mut self,
        tid: TargetId,
        engine: &mut Engine<'a, ErasedMsg>,
        _event: &Event<Self::Message>,
        _orbit: &OrbitLoop,
    ) -> bool {
        self.ensure_texture_loaded(&tid, engine)
    }

    fn view(&self, tid: &TargetId) -> Element<Self::Message> {
        use Length::Grow;

        let Some(tex) = self.textures.get(tid) else {
            return Container::new(vec![Text::new("No image", 32.0).einto()])
                .size(Size::new(Grow, Grow))
                .einto();
        };

        let img = Image::new(Size::new(Grow, Grow), *tex).einto();

        Container::new(vec![img])
            .size(Size::new(Grow, Grow))
            .einto()
    }
}

orbit_plugin! {
    module = Wallpaper,
    manifest = {
        name: "wallpaper",
        commands: ["next"],
        options: Options::Layer(LayerOptions {
            layer: Layer::Background,
            size: Size::new(0, 0),
            anchors: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: -1,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            namespace: Some("wallpaper".to_string()),
            output: Some(OutputSet::All),
        }),
    },
}
