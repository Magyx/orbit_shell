use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use calloop::channel::{self as loop_channel, Channel};
use orbit_api::{
    Engine, ErasedMsg, SettingsOutcome,
    runtime::erased::{erase_element, map_element},
};
use orbit_common::config::Config;
use ui::{
    graphics::TargetId,
    model::Size,
    sctk::{Options, RawWaylandHandles, SctkEvent, SurfaceId, XdgOptions},
    theme::Theme,
    widget::{Element, Length},
};
use yaml_serde::Value;

use crate::{
    event::SettingsEvent,
    module_manager::ModuleManager,
    sctk::{CreatedSurface, SctkApp},
};

const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(1500);

struct ModuleRow {
    name: String,
    label: String,
    enabled: bool,
}

#[derive(Default)]
struct SettingsState {
    size: Size<Length>,
    applied: Option<Instant>,
    current_tab: u8,

    working: Config,
    modules: Vec<ModuleRow>,
    pending: HashMap<String, Instant>,
}

struct SettingsCtx<'a> {
    state: &'a mut SettingsState,
    modules: &'a ModuleManager,
}

#[derive(Clone, Debug)]
enum Msg {
    Tab(u8),

    ToggleModule(String),
    ModuleMessage { module: String, inner: ErasedMsg },

    Apply,
    AppliedNotificationClicked,
    Cancel,
}

fn module_loaded(modules: &ModuleManager, name: &str) -> bool {
    modules
        .find_by_name(name)
        .is_some_and(|(_, m)| m.is_loaded())
}

fn settings_update(
    _engine: &mut Engine,
    event: &orbit_api::Event<ErasedMsg>,
    ctx: &mut SettingsCtx<'_>,
    tx: &loop_channel::Sender<SettingsEvent>,
) -> bool {
    match event {
        &orbit_api::Event::Resized { size } => {
            // _engine.toggle_debug();
            ctx.state.size = Size::new(
                Length::Fixed(size.width as i32),
                Length::Fixed(size.height as i32),
            );
            true
        }
        orbit_api::Event::Message(erased) => {
            let Some(msg) = erased.message::<Msg>() else {
                return false;
            };
            match msg {
                Msg::Tab(tab) => {
                    ctx.state.current_tab = tab;
                    true
                }
                Msg::ToggleModule(name) => {
                    let next = !ctx
                        .state
                        .working
                        .modules
                        .get(&name)
                        .copied()
                        .unwrap_or(false);
                    ctx.state.working.modules.insert(name.clone(), next);
                    if let Some(row) = ctx.state.modules.iter_mut().find(|r| r.name == name) {
                        row.enabled = next;
                    }
                    let _ = tx.send(SettingsEvent::Change(ctx.state.working.clone()));
                    true
                }
                Msg::ModuleMessage { module, inner } => {
                    let Some((_, info)) = ctx.modules.find_by_name(&module) else {
                        return false;
                    };
                    let slot = ctx
                        .state
                        .working
                        .config
                        .entry(module.clone())
                        .or_insert(Value::Null);
                    let ev = orbit_api::Event::Message(inner);
                    match info.as_ref().settings_update(slot, &ev) {
                        SettingsOutcome::Changed => {
                            ctx.state.pending.insert(module, Instant::now());
                            true
                        }
                        SettingsOutcome::Unchanged => true,
                        SettingsOutcome::Ignored => false,
                    }
                }
                Msg::Apply => {
                    if !ctx.state.pending.is_empty() {
                        ctx.state.pending.clear();
                        let _ = tx.send(SettingsEvent::Change(ctx.state.working.clone()));
                    }
                    let _ = tx.send(SettingsEvent::Apply);
                    ctx.state.applied = Some(Instant::now());
                    true
                }
                Msg::AppliedNotificationClicked => {
                    ctx.state.applied = None;
                    true
                }
                Msg::Cancel => {
                    ctx.state.pending.clear();
                    let _ = tx.send(SettingsEvent::Cancel);
                    false
                }
            }
        }
        _ => false,
    }
}

fn settings_view(ctx: &SettingsCtx<'_>, theme: &Theme) -> Element<ErasedMsg> {
    use ui::{
        el,
        model::{Color, Vec4},
        widget::{Button, Column, Length, Overlay, Rectangle, Row, Scrollable, Spacer, Text},
    };
    let state = &*ctx.state;
    let mut side_bar = Column::new(el![
        Text::h2("Settings"),
        Button::new_with(Text::body("Modules"))
            .size(Size::new(Length::Grow, Length::Fit))
            .color(theme.bg)
            .on_press(Msg::Tab(0)),
    ])
    .spacing(12)
    .padding(Vec4::splat(16));

    for (i, row) in state.modules.iter().enumerate() {
        if module_loaded(ctx.modules, &row.name) {
            side_bar.push(
                Button::new_with(Text::body(row.label.clone()))
                    .size(Size::new(Length::Grow, Length::Fit))
                    .color(theme.bg)
                    .on_press(Msg::Tab(i as u8 + 1)),
            );
        } else {
            side_bar.push(
                Button::new_with(
                    Text::label(format!("{}  (disabled)", row.label)).wrap(ui::model::Wrap::None),
                )
                .size(Size::new(Length::Grow, Length::Fit))
                .color(Color::rgba(35, 35, 40, 160)),
            );
        }
    }

    let page: Element<Msg> = if state.current_tab == 0 {
        let mut col = Column::new(el![Text::h3("Modules")])
            .size(Size::splat(Length::Grow))
            .padding(Vec4::splat(16));

        for row in &state.modules {
            let state = if row.enabled { "on" } else { "off" };
            let fill = if row.enabled {
                Color::rgba(60, 120, 220, 220)
            } else {
                Color::rgba(45, 45, 52, 220)
            };
            let button = Button::new_with(Text::body(format!("{}    [{}]", row.name, state)))
                .size(Size::new(Length::Grow, Length::Fit))
                .color(fill)
                .on_press(Msg::ToggleModule(row.name.clone()));
            col.push(
                Row::new(el![button])
                    .size(Size::new(Length::Grow, Length::Fit))
                    .padding(Vec4::splat(6)),
            );
        }
        col.into()
    } else {
        module_page(ctx, theme)
    };

    let app = Row::new(el![
        Scrollable::new(side_bar).size(Size::new(Length::Fit, Length::Grow)),
        Rectangle::new(Size::new(Length::Fixed(1), Length::Grow), theme.on_bg),
        page
    ])
    .size(state.size);

    let notification_zone: Element<Msg> = if state
        .applied
        .is_some_and(|i| i.elapsed() < Duration::from_mins(1))
    {
        let color = Color::rgb(35, 197, 82);
        Column::new(el![
            Spacer::new(Size::splat(Length::Grow)),
            Row::new(el![Button::new_with(
                Text::label("New config applied").wrap(ui::model::Wrap::None)
            )
            .color(color)
            .hover_color(color)
            .pressed_color(color)
            .on_press(Msg::AppliedNotificationClicked)])
            .padding(Vec4::splat(8))
        ])
        .size(Size::splat(Length::Grow))
        .into()
    } else {
        Spacer::new(Size::splat(Length::Grow)).into()
    };
    let overlay = Row::new(el![
        notification_zone,
        Column::new(el![
            Spacer::new(Size::splat(Length::Grow)),
            Row::new(el![
                Button::new_with(Text::body("Cancel"))
                    .color(Color::rgba(45, 45, 52, 220))
                    .hover_color(Color::rgba(220, 100, 20, 220))
                    .border()
                    .on_press(Msg::Cancel),
                Spacer::new(Size::new(Length::Fixed(8), Length::Fit)),
                Button::new_with(Text::body("Apply"))
                    .border()
                    .on_press(Msg::Apply)
            ])
            .padding(Vec4::splat(8))
            .size(Size::new(Length::Grow, Length::Fit))
        ])
        .size(Size::new(Length::Fit, Length::Grow))
    ])
    .size(state.size);

    erase_element(Overlay::new(el![app, overlay]).into())
}

fn module_page(ctx: &SettingsCtx<'_>, theme: &Theme) -> Element<Msg> {
    use ui::{
        el,
        model::Vec4,
        widget::{Column, Length, Text},
    };

    let fallback = |label: &str, note: &str| -> Element<Msg> {
        Column::new(el![Text::h3(label.to_owned()), Text::body(note.to_owned())])
            .size(Size::splat(Length::Grow))
            .padding(Vec4::splat(16))
            .into()
    };

    let Some(row) = ctx.state.modules.get((ctx.state.current_tab - 1) as usize) else {
        return fallback("Settings", "No module selected.");
    };
    let Some((_, info)) = ctx.modules.find_by_name(&row.name) else {
        return fallback(&row.label, "Module not found.");
    };
    if !info.is_loaded() {
        return fallback(&row.label, "Enable this module to configure it.");
    }

    let name = row.name.clone();
    let slot = ctx
        .state
        .working
        .config
        .get(&name)
        .cloned()
        .unwrap_or(Value::Null);
    let page = info.as_ref().settings_view(&slot, theme);
    map_element(page, move |inner| Msg::ModuleMessage {
        module: name.clone(),
        inner,
    })
}

pub struct SettingsApp {
    target: Option<(TargetId, SurfaceId)>,
    pending_sid: Option<SurfaceId>,

    tx: loop_channel::Sender<SettingsEvent>,
    state: SettingsState,
}

fn pretty_label(key: &str) -> String {
    let mut out = String::new();
    for (i, word) in key.split('_').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut ch = word.chars();
        if let Some(f) = ch.next() {
            out.extend(f.to_uppercase());
            out.push_str(ch.as_str());
        }
    }
    out
}

fn get_modules(config: &Config, modules: &ModuleManager) -> Vec<ModuleRow> {
    let mut modules: Vec<ModuleRow> = modules
        .modules()
        .map(|m| {
            let &enabled = config.modules.get(&m.name).unwrap_or(&false);
            ModuleRow {
                label: pretty_label(&m.name),
                name: m.name.clone(),
                enabled,
            }
        })
        .collect();

    modules.sort_by(|a, b| a.name.cmp(&b.name));
    modules
}

impl SettingsApp {
    pub fn new() -> (Channel<SettingsEvent>, Self) {
        let (tx, rx) = loop_channel::channel::<SettingsEvent>();
        (
            rx,
            Self {
                target: None,
                pending_sid: None,
                tx,
                state: SettingsState::default(),
            },
        )
    }

    fn __render(
        &mut self,
        tid: TargetId,
        engine: &mut Engine<'_>,
        modules: &ModuleManager,
        need: bool,
        theme: Theme,
    ) {
        let mut ctx = SettingsCtx {
            state: &mut self.state,
            modules,
        };
        if let Err(e) =
            engine.render_if_needed(&tid, need, &|_, s| settings_view(s, &theme), &mut ctx)
        {
            tracing::error!("settings app render failed: {e:?}");
        }
    }

    pub fn change_config(
        &mut self,
        engine: &mut Engine<'_>,
        modules: &ModuleManager,
        working: Config,
    ) {
        self.state.modules = get_modules(&working, modules);
        self.state.working = working;

        if let Some((tid, _)) = self.target {
            self.__render(tid, engine, modules, true, *engine.theme());
        }
    }
    pub fn apply_external_change(
        &mut self,
        engine: &mut Engine<'_>,
        modules: &ModuleManager,
        new: &Config,
    ) {
        for (k, v) in &new.modules {
            self.state.working.modules.entry(k.clone()).or_insert(*v);
        }
        for (k, v) in &new.config {
            if !self.state.pending.contains_key(k) {
                self.state.working.config.insert(k.clone(), v.clone());
            }
        }
        self.state.modules = get_modules(&self.state.working, modules);
        if let Some((tid, _)) = self.target {
            self.__render(tid, engine, modules, true, *engine.theme());
        }
    }

    pub fn has_pending(&self) -> bool {
        !self.state.pending.is_empty()
    }
    pub fn flush_debounced(&mut self, now: Instant) {
        if !self.has_pending() {
            return;
        }
        let latest = self.state.pending.values().copied().max();
        if latest.is_some_and(|t| now.duration_since(t) >= PREVIEW_DEBOUNCE) {
            self.state.pending.clear();
            let _ = self
                .tx
                .send(SettingsEvent::Change(self.state.working.clone()));
        }
    }

    pub fn is_shown(&self) -> bool {
        self.target.is_some()
    }
    pub fn owns_surface(&self, sid: SurfaceId) -> bool {
        self.target.is_some_and(|(_, s)| s == sid) || self.pending_sid.is_some_and(|s| s == sid)
    }
    pub fn show(&mut self, sctk: &mut SctkApp, modules: &ModuleManager, working: Config) {
        if !self.is_shown() {
            let opts = Options::Xdg(XdgOptions {
                size: Size::new(800, 600),
                app_id: Some("orbit-settings".into()),
                title: "Orbit Settings".into(),
                decorations: ui::sctk::WindowDecorations::ServerDefault,
                output: None,
            });

            if let CreatedSurface::Single(sid) = sctk.create_surfaces(opts) {
                self.pending_sid = Some(sid);
            }
        }

        self.state = SettingsState {
            size: Size::new(Length::Fixed(800), Length::Fixed(600)),
            applied: None,
            current_tab: 0,
            modules: get_modules(&working, modules),
            working,
            pending: Default::default(),
        };
    }
    pub fn try_attach_pending(
        &mut self,
        engine: &mut Engine<'_>,
        sctk: &mut SctkApp,
        sid: SurfaceId,
    ) {
        let Some(sid) = self.pending_sid.take_if(|s| *s == sid) else {
            return;
        };
        let Some(rec) = sctk.state.surfaces.get(&sid) else {
            return;
        };
        let handles = RawWaylandHandles::new(&sctk.conn, &rec.wl_surface);
        let sf = rec.scale_factor.max(1) as f64;
        let phys = Size::new(
            rec.size.width * rec.scale_factor.max(1) as u32,
            rec.size.height * rec.scale_factor.max(1) as u32,
        );
        let tid = engine.attach_target(std::sync::Arc::new(handles), phys, sf);
        self.target = Some((tid, sid));
    }
    pub fn remove_sid(&mut self, engine: &mut Engine<'_>, sctk: &mut SctkApp, sid: SurfaceId) {
        self.pending_sid.take_if(|s| *s == sid);

        if let Some((tid, _)) = self.target.take_if(|(_, s)| *s == sid) {
            engine.detach_target(&tid);
            sctk.state.remove_surface_by_surface_id(sid);
        }
    }
    // FIX: currently don't know how to clean up after the window is closed
    // pub fn hide(&mut self, engine: &mut Engine<'_>, sctk: &mut SctkApp) {
    //     let mut sids = Vec::new();
    //     if let Some(sid) = self.pending_sid.take() {
    //         sids.push(sid);
    //     }
    //     if let Some((tid, sid)) = self.target.take() {
    //         engine.detach_target(&tid);
    //         sids.push(sid);
    //     }
    //     sctk.destroy_surfaces(&sids);
    //     self.state = SettingsState::default();
    // }

    pub fn handle_platform_event(
        &mut self,
        engine: &mut Engine<'_>,
        modules: &ModuleManager,
        event: &SctkEvent,
    ) {
        // TODO: doesn't actually seem to be sent when the settings app is killed
        if matches!(event, SctkEvent::Closed) {
            let _ = self.tx.send(SettingsEvent::Cancel);
            return;
        }

        let Some((tid, sid)) = self.target else {
            return;
        };
        if Some(sid) != event.surface_id() {
            return;
        }

        let tx = self.tx.clone();
        let mut ctx = SettingsCtx {
            state: &mut self.state,
            modules,
        };
        engine.handle_platform_event(&tid, event, &mut settings_update, &mut ctx, &tx);
    }

    pub fn render(&mut self, engine: &mut Engine<'_>, modules: &ModuleManager) {
        if let Some((tid, _)) = self.target {
            let tx = self.tx.clone();
            let mut ctx = SettingsCtx {
                state: &mut self.state,
                modules,
            };
            let need = engine.poll(&tid, &mut settings_update, &mut ctx, &tx);
            self.__render(tid, engine, modules, need, *engine.theme());
        }
    }
}
