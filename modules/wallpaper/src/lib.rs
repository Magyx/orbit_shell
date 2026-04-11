use rand::seq::SliceRandom;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use orbit_api::{
    Engine, Event, OrbitModule, Subscription, Task, orbit_plugin,
    ui::{
        el,
        graphics::TargetId,
        model::{Family, Size},
        render::texture::TextureHandle,
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{ContentFit, Element, Image, Length, Overlay, Rectangle, Text},
    },
};

use config::*;

mod config;

#[derive(Clone, Debug)]
pub enum Msg {
    Tick,
    Cycle,
}

#[derive(Debug)]
pub struct PerTarget {
    size: Size<u32>,
    file: PathBuf,
    tex: TextureHandle,
}

#[derive(Default, Debug)]
struct UsedWidgets {
    clock: bool,
}

#[derive(Default, Debug)]
pub struct Wallpaper {
    widgets: UsedWidgets,

    cfg: Config,
    targets: HashMap<TargetId, PerTarget>,
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
        let mut rng = rand::rng();

        let mut candidates: Vec<PathBuf> = all
            .into_iter()
            .filter(|p| !used.contains(p.as_path()))
            .collect();

        if candidates.is_empty() {
            candidates = self.targets.values().map(|t| t.file.clone()).collect();
        }

        candidates.shuffle(&mut rng);
        candidates.into_iter().next()
    }

    fn load_texture<'a>(
        file: &Path,
        width: u32,
        height: u32,
        engine: &mut Engine<'a>,
    ) -> Option<TextureHandle> {
        if let Ok(reader) = image::ImageReader::open(file)
            && let Ok(mut img) = reader.decode()
        {
            img = img.resize_to_fill(width, height, image::imageops::FilterType::Nearest);
            let rgba = img.to_rgba8();
            let handle = engine.load_texture_rgba8(width, height, rgba.as_raw());
            Some(handle)
        } else {
            None
        }
    }

    fn ensure_texture_loaded(&mut self, tid: &TargetId, engine: &mut Engine<'_>) -> bool {
        if self.targets.contains_key(tid) || !self.cfg.source.exists() {
            return false;
        }
        let Some(path) = self.pick_random_image_unique(&self.cfg.source) else {
            return false;
        };
        let Some(globals) = engine.globals(tid) else {
            return false;
        };
        let (w, h) = (
            globals.window_size[0].ceil() as u32,
            globals.window_size[1].ceil() as u32,
        );
        if w == 0 || h == 0 {
            return false;
        }
        let Some(tex) = Self::load_texture(&path, w, h, engine) else {
            return false;
        };
        self.targets.insert(
            *tid,
            PerTarget {
                size: Size::new(w, h),
                tex,
                file: path,
            },
        );
        true
    }

    fn get_min_clock_duration(&self) -> Duration {
        self.cfg
            .widgets
            .iter()
            .filter_map(|w| w.clock_duration())
            .min()
            .unwrap_or(Duration::from_hours(1))
    }
}

impl OrbitModule for Wallpaper {
    type Config = Config;
    type Message = Msg;

    fn cleanup<'a>(&mut self, engine: &mut Engine<'a>) {
        for (_, target) in self.targets.drain() {
            engine.unload_texture(target.tex);
        }
    }

    fn validate_config(cfg: Self::Config) -> Result<(), String> {
        let mut errors = Vec::new();

        for widget in cfg.widgets.into_iter() {
            match widget {
                WidgetConfig::Clock {
                    font_size,
                    time_format,
                    ..
                } => {
                    if font_size <= 0.0 {
                        errors.push("- font_size must be > 0".into());
                    }

                    if let Err(e) = chrono::format::StrftimeItems::new(&time_format).parse() {
                        errors.push(format!("- invalid time_format `{time_format}`: {e}"));
                    }
                }
                WidgetConfig::None => (),
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    fn apply_config<'a>(
        &mut self,
        engine: &mut Engine<'a>,
        config: Self::Config,
        _options: &mut orbit_api::ui::sctk::Options,
    ) -> bool {
        if self.cfg.source != config.source {
            self.cleanup(engine);
        }
        self.cfg = config;

        self.widgets.clock = self
            .cfg
            .widgets
            .iter()
            .any(|w| matches!(w, WidgetConfig::Clock { .. }));

        false
    }

    fn update<'a>(
        &mut self,
        tid: TargetId,
        engine: &mut Engine<'a>,
        event: &Event<Self::Message>,
    ) -> Task<Msg> {
        match event {
            &Event::Resized { size } => {
                if let Some(target) = self.targets.get_mut(&tid) {
                    if target.size == size {
                        return Task::None;
                    }
                    if let Some(tex) =
                        Self::load_texture(&target.file, size.width, size.height, engine)
                    {
                        let old = std::mem::replace(&mut target.tex, tex);
                        target.size = size;
                        engine.unload_texture(old);
                    }
                } else {
                    self.ensure_texture_loaded(&tid, engine);
                }
                Task::RedrawTarget
            }
            Event::Message(Msg::Tick) => {
                if self.widgets.clock {
                    Task::RedrawModule
                } else {
                    Task::None
                }
            }
            Event::Message(Msg::Cycle) => {
                let targets_to_reload: Vec<_> = self.targets.drain().collect();
                for (tid, target) in targets_to_reload {
                    engine.unload_texture(target.tex);
                    self.ensure_texture_loaded(&tid, engine);
                }
                Task::RedrawModule
            }
            _ => Task::None,
        }
    }

    fn view(&self, tid: &TargetId) -> Element<Self::Message> {
        fn place_widget<E>(target: &PerTarget, element: E, x: f32, y: f32, on: &mut Overlay<Msg>)
        where
            E: Into<Element<Msg>>,
        {
            on.push(
                element,
                (target.size.width as f32 * x.clamp(0.0, 1.0)).ceil() as i32,
                (target.size.height as f32 * y.clamp(0.0, 1.0)).ceil() as i32,
            );
        }
        use Length::Grow;

        let Some(target) = self.targets.get(tid) else {
            return Rectangle::placeholder().into();
        };

        let mut view = Overlay::new(el![
            Image::new(Size::splat(Grow), target.tex).fit(ContentFit::Cover)
        ])
        .size(Size::splat(Grow));

        for widget in self.cfg.widgets.iter() {
            match widget {
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
                    place_widget(target, text, *x, *y, &mut view);
                }
                WidgetConfig::None => (),
            }
        }

        view.into()
    }

    fn subscriptions(&self) -> Subscription<Self::Message> {
        let cycle: Duration = self
            .cfg
            .cycle
            .parse::<humantime::Duration>()
            .map(Into::into)
            .unwrap_or(Duration::from_secs(3600));
        let mut subs = vec![Subscription::Interval {
            every: cycle,
            message: Msg::Cycle,
        }];

        if self.widgets.clock {
            subs.push(Subscription::SyncedInterval {
                every: self.get_min_clock_duration(),
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
        commands: [("next", Msg::Cycle)],
        options: Options::Layer(LayerOptions {
            layer: Layer::Background,
            size: Size::new(0, 0),
            anchors: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
            exclusive_zone: -1,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            namespace: Some("orbit-wallpaper".to_string()),
            output: Some(OutputSet::All),
        }),
    },
}
