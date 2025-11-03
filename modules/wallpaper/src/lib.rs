use rand::seq::SliceRandom;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use orbit_api::{
    ErasedMsg, Event, OrbitCtl, OrbitModule, Subscription, orbit_plugin,
    ui::{
        el,
        graphics::{Engine, TargetId},
        model::Size,
        render::texture::TextureHandle,
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{Element, Image, Length, Overlay, Rectangle, Text},
    },
};

use crate::config::{Config, WidgetConfig};

mod config;

#[derive(Clone, Debug)]
pub enum Msg {
    None,
    Tick,
    Cycle,
}

#[derive(Debug)]
pub struct PerTarget {
    size: Option<Size<u32>>,
    file: PathBuf,
    tex: TextureHandle,
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
                    size: None,
                    tex: handle,
                    file: p,
                },
            );
            return true;
        }
        false
    }

    fn any_clock(&self) -> bool {
        self.cfg
            .widgets
            .iter()
            .any(|w| matches!(w, WidgetConfig::Clock { .. }))
    }

    fn clock_needs_seconds(&self) -> bool {
        self.cfg.widgets.iter().any(|w| match w {
            WidgetConfig::Clock { time_format, .. } => time_format.contains("%S"),
        })
    }

    fn place_widget<E>(&self, tid: &TargetId, element: E, x: f32, y: f32, on: &mut Overlay<Msg>)
    where
        E: Into<Element<Msg>>,
    {
        if let Some(target) = self.targets.get(tid)
            && let Some(t_size) = target.size
        {
            on.push(
                element,
                (t_size.width as f32 * x.clamp(0.0, 1.0)).ceil() as i32,
                (t_size.height as f32 * y.clamp(0.0, 1.0)).ceil() as i32,
            );
        }
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
        if self.cfg.source != config.source {
            Self::cleanup(self, engine);
        }
        if self.cfg != config {
            self.cfg = config;
        }
        false
    }

    fn update<'a>(
        &mut self,
        tid: TargetId,
        engine: &mut Engine<'a, ErasedMsg>,
        event: &Event<Self::Message>,
        _orbit: &OrbitCtl,
    ) -> bool {
        let mut needs_redraw = self.ensure_texture_loaded(&tid, engine);

        match event {
            Event::Resized { size } => {
                if let Some(target) = self.targets.get_mut(&tid) {
                    target.size = Some(*size);
                    needs_redraw = true;
                }
            }
            Event::Message(Msg::Cycle) => {
                if let Some(target) = self.targets.remove(&tid) {
                    engine.unload_texture(target.tex);
                    let loaded = self.ensure_texture_loaded(&tid, engine);
                    if loaded {
                        self.targets.get_mut(&tid).expect("just loaded").size = target.size;
                    }
                    needs_redraw |= loaded;
                }
            }
            Event::Message(Msg::Tick) => {
                if self.any_clock() {
                    needs_redraw = true;
                }
            }
            _ => {}
        }

        needs_redraw
    }

    fn view(&self, tid: &TargetId) -> Element<Self::Message> {
        use Length::Grow;

        let Some(target) = self.targets.get(tid) else {
            return Rectangle::placeholder().into();
        };

        let mut view =
            Overlay::new(el![Image::new(Size::splat(Grow), target.tex)]).size(Size::splat(Grow));

        for w in self.cfg.widgets.iter() {
            match w {
                WidgetConfig::Clock {
                    x,
                    y,
                    font_size,
                    time_format,
                } => {
                    let time = chrono::Local::now().format(time_format).to_string();
                    let text = Text::new(time, *font_size);
                    self.place_widget(tid, text, *x, *y, &mut view);
                }
            }
        }

        view.into()
    }

    fn subscriptions(&self) -> Subscription<Self::Message> {
        let every_wall = self
            .cfg
            .cycle
            .parse::<humantime::Duration>()
            .unwrap_or(humantime::Duration::from(std::time::Duration::from_secs(
                3600,
            )))
            .into();

        let mut subs = vec![Subscription::Interval {
            every: every_wall,
            message: Msg::Cycle,
        }];

        if self.any_clock() {
            let every_clock = if self.clock_needs_seconds() {
                Duration::from_secs(1)
            } else {
                Duration::from_secs(60)
            };
            subs.push(Subscription::Interval {
                every: every_clock,
                message: Msg::Tick,
            });
        }

        Subscription::Batch(subs)
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
