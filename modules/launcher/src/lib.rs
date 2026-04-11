use std::sync::Arc;

use orbit_api::{
    Engine, Event, OrbitModule, Task, orbit_plugin,
    ui::{
        el,
        event::{KeyEvent, LogicalKey},
        graphics::TargetId,
        model::{Color, Size, Vec4, Wrap},
        render::texture::{Atlas, TextureHandle},
        sctk::{Anchor, KeyboardInteractivity, Layer, LayerOptions, Options, OutputSet},
        widget::{Column, Element, Image, Length, Rectangle, Row, Scrollable, Spacer, Text},
    },
};

use crate::config::Config;
mod config;
mod helpers;

#[derive(Debug, Clone)]
pub struct RawEntry {
    pub name: String,
    pub description: String,
    pub exec: String,
    pub icon_name: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone)]
struct AppEntry {
    name: String,
    description: String,
    exec: String,
    icon_name: String,
    icon: Option<TextureHandle>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    ScannedApps(Arc<Vec<RawEntry>>),
    Results(Vec<usize>),
    IconLoaded(usize, u32, u32, Arc<Vec<u8>>),
    Refresh,
    Launched,
}

#[derive(Default)]
pub struct Launcher {
    cfg: Config,
    query: String,
    apps: Arc<Vec<RawEntry>>,
    entries: Vec<AppEntry>,
    /// Indices into `entries` for the current result set.
    /// When query is empty this is all entries in order.
    results: Vec<usize>,
    selected: usize,
    atlas: Option<Atlas>,
}

impl Launcher {
    fn active_exec(&self) -> Option<&str> {
        self.results
            .get(self.selected)
            .and_then(|&i| self.entries.get(i))
            .map(|e| e.exec.as_str())
    }

    fn clamp_selection(&mut self) {
        if self.results.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.results.len() - 1);
        }
    }

    fn select_next(&mut self, step: usize) {
        if !self.results.is_empty() {
            self.selected = (self.selected + step).min(self.results.len() - 1);
        }
    }

    fn select_prev(&mut self, step: usize) {
        self.selected = self.selected.saturating_sub(step);
    }

    fn show_all(&mut self) {
        self.results = (0..self.entries.len()).collect();
        self.selected = 0;
    }

    fn rebuild_entries_from_apps(&mut self) {
        use std::collections::HashMap;
        let old: HashMap<String, Option<TextureHandle>> = self
            .entries
            .drain(..)
            .map(|e| (e.name.clone(), e.icon))
            .collect();

        self.entries = self
            .apps
            .iter()
            .map(|raw| AppEntry {
                name: raw.name.clone(),
                description: raw.description.clone(),
                exec: raw.exec.clone(),
                icon_name: raw.icon_name.clone(),
                icon: old.get(&raw.name).and_then(|h| *h),
            })
            .collect();
    }

    fn ensure_atlas(&mut self, engine: &mut Engine<'_>) {
        if self.atlas.is_none() {
            self.atlas = Some(engine.create_atlas(512, 512));
        }
    }

    fn destroy_atlas(&mut self, engine: &mut Engine<'_>) {
        if let Some(mut atlas) = self.atlas.take() {
            engine.destroy_atlas(&mut atlas);
        }
        for entry in &mut self.entries {
            entry.icon = None;
        }
    }
}

impl OrbitModule for Launcher {
    type Config = Config;
    type Message = Msg;

    fn cleanup<'a>(&mut self, engine: &mut Engine<'a>) {
        self.query.clear();
        self.results.clear();
        self.selected = 0;
        self.destroy_atlas(engine);
        self.entries.clear();
        self.apps = Arc::new(Vec::new());
    }

    fn validate_config(cfg: Self::Config) -> Result<(), String> {
        if cfg.width < 200 {
            return Err("width must be at least 200".into());
        }
        if cfg.height < 100 {
            return Err("height must be at least 100".into());
        }
        if cfg.max_results == 0 {
            return Err("max_results must be at least 1".into());
        }
        if cfg.icon_size < 8 || cfg.icon_size > 256 {
            return Err("icon_size must be between 8 and 256".into());
        }
        if !matches!(cfg.position.as_str(), "top" | "center" | "bottom") {
            return Err("position must be one of: top, center, bottom".into());
        }
        Ok(())
    }

    fn apply_config<'a>(
        &mut self,
        _engine: &mut Engine<'a>,
        config: Self::Config,
        options: &mut Options,
    ) -> bool {
        let size_changed = self.cfg.width != config.width || self.cfg.height != config.height;
        let pos_changed = self.cfg.position != config.position;
        self.cfg = config;
        if size_changed || pos_changed {
            if let Options::Layer(layer) = options {
                layer.size.width = self.cfg.width;
                layer.size.height = self.cfg.height;
                layer.anchors = config::anchors_for_position(&self.cfg.position);
            }
            return true;
        }
        false
    }

    fn update<'a>(
        &mut self,
        _tid: TargetId,
        engine: &mut Engine<'a>,
        event: &Event<Self::Message>,
    ) -> Task<Msg> {
        match event {
            Event::RedrawRequested => {
                if self.apps.is_empty() {
                    self.ensure_atlas(engine);
                    Task::spawn(async { helpers::scan_desktop_files().await })
                } else {
                    Task::None
                }
            }

            Event::Key(KeyEvent {
                state: orbit_api::ui::event::KeyState::Pressed,
                logical_key: key,
                ..
            }) => match key {
                LogicalKey::Backspace => {
                    self.query.pop();
                    self.selected = 0;
                    if self.query.is_empty() {
                        self.show_all();
                        Task::RedrawTarget
                    } else {
                        let query = self.query.clone();
                        let apps = Arc::clone(&self.apps);
                        let max = self.cfg.max_results;
                        Task::spawn(async move { helpers::search(&apps, &query, max) })
                    }
                }

                LogicalKey::ArrowDown => {
                    self.select_next(1);
                    Task::RedrawTarget
                }
                LogicalKey::ArrowUp => {
                    self.select_prev(1);
                    Task::RedrawTarget
                }
                LogicalKey::Tab => {
                    self.select_next(1);
                    Task::RedrawTarget
                }

                LogicalKey::Enter => {
                    if let Some(exec) = self.active_exec().map(str::to_owned) {
                        self.query.clear();
                        self.results.clear();
                        self.selected = 0;
                        Task::spawn(async move { helpers::launch_app(exec).await })
                    } else {
                        Task::None
                    }
                }

                LogicalKey::Escape => {
                    self.query.clear();
                    self.results.clear();
                    self.selected = 0;
                    Task::ExitModule
                }

                LogicalKey::Character(_) | LogicalKey::Space => {
                    let c = if let LogicalKey::Character(ch) = key {
                        ch.as_str()
                    } else {
                        " "
                    };

                    match c {
                        "\x0A" => {
                            self.select_next(1);
                            return Task::RedrawTarget;
                        } // ctrl+j
                        "\x0B" => {
                            self.select_prev(1);
                            return Task::RedrawTarget;
                        } // ctrl+k
                        "\x04" => {
                            self.select_next(self.cfg.max_results / 2);
                            return Task::RedrawTarget;
                        } // ctrl+d
                        "\x15" => {
                            self.select_prev(self.cfg.max_results / 2);
                            return Task::RedrawTarget;
                        } // ctrl+u
                        "\x0E" => {
                            self.select_next(1);
                            return Task::RedrawTarget;
                        } // ctrl+n
                        "\x10" => {
                            self.select_prev(1);
                            return Task::RedrawTarget;
                        } // ctrl+p
                        "`" => {
                            engine.toggle_debug();
                            return Task::RedrawTarget;
                        }
                        _ => (),
                    }

                    let ch = match c.chars().next() {
                        Some(ch) => ch,
                        _ => return Task::None,
                    };

                    self.query.push(ch);
                    self.selected = 0;
                    let query = self.query.clone();
                    let apps = Arc::clone(&self.apps);
                    let max = self.cfg.max_results;
                    Task::spawn(async move { helpers::search(&apps, &query, max) })
                }

                _ => Task::None,
            },

            Event::Message(Msg::ScannedApps(apps)) => {
                self.apps = Arc::clone(apps);
                self.rebuild_entries_from_apps();
                self.show_all();

                let icon_size = self.cfg.icon_size;
                let tasks: Vec<Task<Msg>> = self
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.icon.is_none() && !e.icon_name.is_empty())
                    .map(|(i, e)| {
                        let icon_name = e.icon_name.clone();
                        Task::spawn(
                            async move { helpers::load_icon(i, &icon_name, icon_size).await },
                        )
                    })
                    .collect();

                Task::batch(tasks)
            }

            Event::Message(Msg::Results(indices)) => {
                self.results = indices.clone();
                self.clamp_selection();
                Task::RedrawTarget
            }

            Event::Message(Msg::IconLoaded(idx, w, h, pixels)) => {
                if *w == 0 || *h == 0 {
                    return Task::None;
                }
                if let Some(entry) = self.entries.get_mut(*idx)
                    && let Some(atlas) = &mut self.atlas
                {
                    entry.icon = engine.load_texture_into_atlas(atlas, *w, *h, pixels.as_slice());
                }
                if self.results.contains(idx) {
                    Task::RedrawTarget
                } else {
                    Task::None
                }
            }

            Event::Message(Msg::Refresh) => {
                self.destroy_atlas(engine);
                self.ensure_atlas(engine);
                self.apps = Arc::new(Vec::new());
                self.entries.clear();
                self.results.clear();
                self.query.clear();
                self.selected = 0;
                Task::spawn(async { helpers::scan_desktop_files().await })
            }

            Event::Message(Msg::Launched) => Task::ExitModule,

            _ => Task::None,
        }
    }

    fn view(&self, _tid: &TargetId) -> Element<Self::Message> {
        let icon_sz = self.cfg.icon_size as i32;

        // ---- Search bar ----
        let prompt = if self.query.is_empty() {
            "  Search applications…".to_owned()
        } else {
            format!("  {}_", self.query)
        };

        let search_bar = Row::new(el![Text::new(prompt, 16.0)])
            .size(Size::new(Length::Grow, Length::Fit))
            .padding(Vec4::new(14, 16, 14, 16))
            .color(Color::rgba(40, 40, 45, 255));

        let divider = Rectangle::new(
            Size::new(Length::Grow, Length::Fixed(1)),
            Color::rgba(70, 70, 80, 180),
        );

        // ---- Results ----
        let mut results_col =
            Column::new::<Vec<_>, Element<Msg>>(el!()).size(Size::new(Length::Grow, Length::Fit));

        for (row_idx, &entry_idx) in self.results.iter().enumerate() {
            let Some(entry) = self.entries.get(entry_idx) else {
                continue;
            };

            let is_selected = row_idx == self.selected;
            let bg = if is_selected {
                Color::rgba(60, 120, 220, 200)
            } else {
                Color::rgba(0, 0, 0, 0)
            };

            let icon_el: Element<Msg> = if let Some(handle) = entry.icon {
                Image::new(Size::splat(Length::Fixed(icon_sz)), handle).into()
            } else {
                Rectangle::new(
                    Size::splat(Length::Fixed(icon_sz)),
                    Color::rgba(70, 70, 80, 180),
                )
                .into()
            };

            let name_color = Color::rgba(230, 230, 235, 255);
            let desc_color = if is_selected {
                Color::rgba(190, 205, 240, 200)
            } else {
                Color::rgba(130, 130, 145, 200)
            };

            let text_col = Column::new(el![
                Text::new(entry.name.clone(), 14.0)
                    .color(name_color)
                    .size(Size::new(Length::Grow, Length::Fit))
                    .wrap(Wrap::None),
                Text::new(entry.description.clone(), 11.0)
                    .color(desc_color)
                    .size(Size::new(Length::Grow, Length::Fit))
                    .wrap(Wrap::None),
            ])
            .size(Size::new(Length::Grow, Length::Fit));

            let row = Row::new(el![
                icon_el,
                Spacer::new(Size::new(Length::Fixed(10), Length::Fit)),
                text_col
            ])
            .size(Size::new(Length::Grow, Length::Fit))
            .padding(Vec4::splat(10))
            .color(bg);

            results_col.push(row);

            // Thin separator between rows, skip after last.
            if row_idx + 1 < self.results.len() {
                results_col.push(Rectangle::new(
                    Size::new(Length::Grow, Length::Fixed(1)),
                    Color::rgba(55, 55, 65, 120),
                ));
            }
        }

        // ---- Body ----
        let body: Element<Msg> = if !self.results.is_empty() {
            Scrollable::new(results_col)
                .size(Size::new(Length::Grow, Length::Fit))
                .into()
        } else if !self.query.is_empty() {
            Column::new(el![
                Spacer::new(Size::splat(Length::Grow)),
                Row::new(el![Text::new("No applications found".to_owned(), 13.0)
                    .color(Color::rgba(130, 130, 145, 200))])
                .size(Size::new(Length::Grow, Length::Fit))
                .padding(Vec4::splat(16)),
                Spacer::new(Size::splat(Length::Grow)),
            ])
            .size(Size::splat(Length::Grow))
            .into()
        } else {
            // Apps not yet loaded.
            Spacer::new(Size::splat(Length::Grow)).into()
        };

        Column::new(el![search_bar, divider, body])
            .size(Size::splat(Length::Grow))
            .color(Color::rgba(22, 22, 26, 240))
            .into()
    }
}

orbit_plugin! {
    module = Launcher,
    manifest = {
        name: "launcher",
        commands: [("refresh", Msg::Refresh)],
        options: Options::Layer(LayerOptions {
            layer: Layer::Overlay,
            size: Size::new(600, 420),
            anchors: Anchor::empty(), // center by default
            exclusive_zone: 0,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            namespace: Some("orbit-launcher".to_string()),
            output: Some(OutputSet::Active),
        }),
        show_on_startup: false,
    },
}
