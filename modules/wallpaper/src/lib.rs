use rand::seq::SliceRandom;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use orbit_api::{
    ErasedMsg, Event, OrbitLoop, OrbitModule, Subscription, orbit_plugin,
    ui::{
        graphics::{Engine, TargetId},
        model::Size,
        render::texture::TextureHandle,
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{Container, Element, Image, Length, Text, Widget as _},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub enum Msg {
    None,
    Tick,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WidgetConfig {
    #[serde(rename = "type")]
    r#type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub source: PathBuf,
    pub widgets: Vec<WidgetConfig>,
}

impl Default for Config {
    fn default() -> Self {
        let home = xdg_user::pictures().unwrap_or_default().unwrap_or_default();
        Self {
            source: home.join("Wallpapers"),
            widgets: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct PerTarget {
    file: PathBuf,
    tex: TextureHandle,
    time: chrono::DateTime<chrono::Local>,
}

#[derive(Default, Debug)]
pub struct Wallpaper {
    targets: HashMap<TargetId, PerTarget>,
    cfg: Config,
}

impl Wallpaper {
    fn is_supported_ext(p: &Path) -> bool {
        p.extension()
            .and_then(|x| x.to_str())
            .map(|x| matches!(&x.to_ascii_lowercase()[..], "jpg" | "jpeg" | "png"))
            .unwrap_or(false)
    }

    fn collect_images_rec(path: &Path, out: &mut Vec<PathBuf>) {
        if path.is_file() {
            if Self::is_supported_ext(path) {
                out.push(path.to_path_buf());
            }
            return;
        }

        let Ok(rd) = fs::read_dir(path) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                Self::collect_images_rec(&p, out);
            } else if p.is_file() && Self::is_supported_ext(&p) {
                out.push(p);
            }
        }
    }

    fn pick_random_image_unique(&self, root: &Path) -> Option<PathBuf> {
        let mut all = Vec::new();
        Self::collect_images_rec(root, &mut all);

        if all.is_empty() {
            return None;
        }

        let used: HashSet<&Path> = self.targets.values().map(|t| t.file.as_path()).collect();

        let mut unused: Vec<PathBuf> = all
            .into_iter()
            .filter(|p| !used.contains(p.as_path()))
            .collect();

        let mut rng = rand::rng();

        if !unused.is_empty() {
            unused.shuffle(&mut rng);
            return unused.pop();
        }

        let mut any: Vec<PathBuf> = self.targets.values().map(|t| t.file.clone()).collect();
        if any.is_empty() {
            let mut fallback = Vec::new();
            Self::collect_images_rec(root, &mut fallback);
            fallback.shuffle(&mut rng);
            fallback.pop()
        } else {
            any.shuffle(&mut rng);
            any.pop()
        }
    }

    fn ensure_texture_loaded<'a>(
        &mut self,
        tid: &TargetId,
        engine: &mut Engine<'a, ErasedMsg>,
    ) -> bool {
        if self.targets.contains_key(tid) || !self.cfg.source.exists() {
            return false;
        }

        let Some(p) = self.pick_random_image_unique(Path::new(&self.cfg.source)) else {
            return false;
        };

        if let Ok(reader) = image::ImageReader::open(&p)
            && let Ok(img) = reader.decode()
        {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let handle = engine.load_texture_rgba8(w, h, rgba.as_raw());
            self.targets.insert(
                *tid,
                PerTarget {
                    tex: handle,
                    file: p,
                    time: chrono::Local::now(),
                },
            );
            return true;
        }
        false
    }
}

impl OrbitModule for Wallpaper {
    type Config = Config;
    type Message = Msg;

    fn cleanup<'a>(&mut self, engine: &mut Engine<'a, ErasedMsg>) {
        for (_, target) in self.targets.drain() {
            engine.unload_texture(target.tex);
        }
    }

    fn apply_config<'a>(
        &mut self,
        engine: &mut Engine<'a, ErasedMsg>,
        config: Self::Config,
        _options: &mut orbit_api::ui::sctk::Options,
    ) -> bool {
        if self.cfg != config {
            Self::cleanup(self, engine);
            self.cfg = config;
        }
        false
    }

    fn update<'a>(
        &mut self,
        tid: TargetId,
        engine: &mut Engine<'a, ErasedMsg>,
        event: &Event<Self::Message>,
        _orbit: &OrbitLoop,
    ) -> bool {
        let mut needs_redraw = self.ensure_texture_loaded(&tid, engine);
        if let Event::Message(Msg::Tick) = event
            && let Some(target) = self.targets.get_mut(&tid)
        {
            target.time = chrono::Local::now();
            needs_redraw = true;
        }

        needs_redraw
    }

    fn view(&self, tid: &TargetId) -> Element<Self::Message> {
        use Length::{Fit, Grow};

        let Some(target) = self.targets.get(tid) else {
            return Container::new(vec![Text::new("No image", 32.0).einto()])
                .size(Size::new(Grow, Grow))
                .einto();
        };

        let img = Image::new(Size::splat(Grow), target.tex).einto();
        let clock = Text::new(target.time.format("%H:%M:%S").to_string(), 32.0)
            .size(Size::splat(Fit))
            .einto();

        Container::new(vec![img, clock])
            .size(Size::splat(Grow))
            .einto()
    }

    fn subscriptions(&self) -> Subscription<Self::Message> {
        Subscription::Interval {
            every: Duration::from_secs(1),
            message: Msg::Tick,
        }
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
