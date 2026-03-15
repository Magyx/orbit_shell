// TODO: better error messages cmon dude
use std::{
    collections::HashMap,
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use calloop::{
    EventLoop, LoopHandle, RegistrationToken, channel as loop_channel,
    futures::{Scheduler, executor},
    timer::{TimeoutAction, Timer},
};
use serde_yml::Value;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use ui::{
    graphics::TargetId,
    render::pipeline::PipelineKey,
    sctk::{SctkEvent, SurfaceId, state::SctkState},
};

use orbit_api::{Engine, ErasedMsg};
use orbit_dbus::DbusEvent;

use crate::{
    api_utils::UnraveledTask,
    config::{Config, ConfigEvent, ConfigInstruction, ConfigWatcher, compare_configs},
    module::ModuleInfo,
    sctk::{CreatedSurface, SctkApp},
};

use {dbus::OrbitdServer, event::Event, module::ModuleId};

mod api_utils;
mod config;
mod dbus;
mod dialog;
mod event;
mod module;
mod sctk;
mod trace;

struct Orbit<'a> {
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,

    dbus_rx: Option<loop_channel::Channel<DbusEvent>>,
    d_server: OrbitdServer,

    config_rx: Option<loop_channel::Channel<ConfigEvent>>,
    config_watcher: ConfigWatcher,

    sctk_rx: Option<loop_channel::Channel<SctkEvent>>,
    sctk: SctkApp,

    config_path: PathBuf,
    config: Config,
    modules: HashMap<ModuleId, ModuleInfo>,
    subs: HashMap<ModuleId, Vec<RegistrationToken>>,
    error_dialog: Vec<(TargetId, SurfaceId)>,
    errors: Vec<String>,

    engine: Engine<'a>,
    by_module: HashMap<ModuleId, Vec<TargetId>>,
    by_surface: HashMap<SurfaceId, (TargetId, ModuleId)>,
    by_target: HashMap<TargetId, (SurfaceId, ModuleId)>,
}

impl<'a> Orbit<'a> {
    fn new(config_path: Option<PathBuf>) -> Result<Self, String> {
        let config_path = config_path.unwrap_or_else(config::xdg_config_home);

        let (tx, rx) = mpsc::channel::<Event>();

        let (sctk_rx, sctk) = SctkApp::new(tx.clone())?;
        let mut engine = Engine::default();

        let mut config = config::load_cfg(&config_path)?;
        let modules =
            Self::discover_and_load_modules(&mut config, &config_path, &mut engine, None)?;
        let modules_len = modules.len();

        let (dbus_rx, d_server) = OrbitdServer::new();
        let (config_rx, config_watcher) = ConfigWatcher::new();

        Ok(Self {
            tx,
            rx,
            dbus_rx: Some(dbus_rx),
            d_server,

            config_rx: Some(config_rx),
            config_watcher,

            sctk_rx: Some(sctk_rx),
            sctk,

            config_path,
            config,
            modules,
            subs: HashMap::new(),
            error_dialog: Vec::new(),
            errors: Vec::new(),

            engine,
            by_module: HashMap::with_capacity(modules_len),
            by_surface: HashMap::with_capacity(modules_len),
            by_target: HashMap::with_capacity(modules_len),
        })
    }

    fn discover_and_load_modules(
        cfg: &mut Config,
        config_path: &Path,
        engine: &mut Engine<'a>,
        prev_module_len: Option<usize>,
    ) -> Result<HashMap<ModuleId, ModuleInfo>, String> {
        match Self::discover_modules(config_path, prev_module_len) {
            Ok(modules) => Self::load_modules(modules, cfg, engine),
            Err(e) => Err(e),
        }
    }

    fn discover_modules(
        config_path: &Path,
        prev_module_len: Option<usize>,
    ) -> Result<Vec<ModuleInfo>, String> {
        const SYSTEM_MODULES: &str = "/usr/lib/orbit/modules";

        let user_dir_opt = config::modules_dir_if_exists(config_path);
        let mut by_name: HashMap<String, PathBuf> = HashMap::new();

        let push_dir = |map: &mut HashMap<String, PathBuf>, dir: &Path| -> Result<(), String> {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries {
                    let path = entry.map_err(|e| e.to_string())?.path();
                    if path.extension().map(|e| e == "so").unwrap_or(false)
                        && let Some(name) = path.file_name().and_then(|n| n.to_str())
                    {
                        map.insert(name.split('.').next().unwrap().to_string(), path);
                    }
                }
            }
            Ok(())
        };

        let _ = push_dir(&mut by_name, Path::new(SYSTEM_MODULES));
        if let Some(user_dir) = user_dir_opt.as_deref() {
            let _ = push_dir(&mut by_name, user_dir);
        }

        let mut items: Vec<(String, PathBuf)> = by_name.into_iter().collect();
        items.sort_by(|(a, _), (b, _)| a.cmp(b));

        let mut modules = Vec::with_capacity(prev_module_len.unwrap_or_default());
        for (name, path) in items {
            modules.push(ModuleInfo::new(name, path));
        }
        Ok(modules)
    }

    fn load_modules(
        modules: Vec<ModuleInfo>,
        cfg: &Config,
        engine: &mut Engine<'a>,
    ) -> Result<HashMap<ModuleId, ModuleInfo>, String> {
        let mut loaded_modules = HashMap::with_capacity(modules.len());
        let mut next_id: u32 = 0;
        for mut module in modules {
            let enabled = cfg.enabled(&module.name);
            if enabled {
                Self::load_module(engine, cfg.get(&module.name), &mut module)?;
                module.toggled = module.as_ref().manifest().show_on_startup;
            } else {
                module.toggled = false;
                module.inner = None;
            }

            let id = ModuleId(next_id);
            next_id = next_id.wrapping_add(1);
            loaded_modules.insert(id, module);
        }

        Ok(loaded_modules)
    }

    fn load_module(
        engine: &mut Engine<'a>,
        map: Option<&serde_yml::Value>,
        module: &mut ModuleInfo,
    ) -> Result<(), String> {
        module.ensure_loaded()?;
        module
            .as_ref()
            .validate_config(map.unwrap_or(&Value::Null))?;

        for (key, factory) in module.as_ref().pipelines() {
            engine.register_pipeline(PipelineKey::Other(key), factory);
        }

        Ok(())
    }

    fn add_subscriptions(
        &mut self,
        tx: &mpsc::Sender<Event>,
        loop_handle: &mut LoopHandle<SctkState>,
        mid: &ModuleId,
        module: &mut ModuleInfo,
    ) {
        if !module.toggled {
            return;
        }

        fn flatten_subs(
            s: orbit_api::Subscription<ErasedMsg>,
            out: &mut Vec<orbit_api::Subscription<ErasedMsg>>,
        ) {
            use orbit_api::Subscription::*;
            match s {
                None => (),
                Batch(v) => {
                    for child in v {
                        flatten_subs(child, out);
                    }
                }
                other => out.push(other),
            }
        }

        let mut subs = Vec::new();
        flatten_subs(module.as_ref().subscriptions(), &mut subs);

        let ui_tx = tx.clone();
        let mut tokens = Vec::new();
        for sub in subs {
            match sub {
                orbit_api::Subscription::Interval { every, message } => {
                    let timer = Timer::from_duration(every);
                    let base = message;
                    let token = loop_handle
                        .insert_source(timer, {
                            let ui_tx = ui_tx.clone();
                            let mid = *mid;
                            move |_deadline: std::time::Instant, _, _| {
                                let _ = ui_tx.send(Event::Ui(event::Ui::Module(
                                    mid,
                                    SctkEvent::message(base.clone_for_send()),
                                )));
                                TimeoutAction::ToDuration(every)
                            }
                        })
                        .expect("insert Timer");
                    tokens.push(token);
                }
                orbit_api::Subscription::Timeout { after, message } => {
                    let timer = Timer::from_duration(after);
                    let base = message;
                    let token = loop_handle
                        .insert_source(timer, {
                            let ui_tx = ui_tx.clone();
                            let mid = *mid;
                            move |_deadline: std::time::Instant, _, _| {
                                let _ = ui_tx.send(Event::Ui(event::Ui::Module(
                                    mid,
                                    SctkEvent::message(base.clone_for_send()),
                                )));
                                TimeoutAction::Drop
                            }
                        })
                        .expect("insert Timer");
                    tokens.push(token);
                }
                orbit_api::Subscription::None | orbit_api::Subscription::Batch(_) => {}
            }
        }
        if !tokens.is_empty() {
            self.subs.entry(*mid).or_default().append(&mut tokens);
        }
    }

    fn remove_subscriptions(&mut self, loop_handle: &mut LoopHandle<SctkState>, mid: ModuleId) {
        if let Some(tokens) = self.subs.remove(&mid) {
            for token in tokens {
                loop_handle.remove(token);
            }
        }
    }

    fn realize_modules(
        &mut self,
        tx: &mpsc::Sender<Event>,
        loop_handle: &mut LoopHandle<SctkState>,
        cfg: &Config,
        modules: &mut HashMap<ModuleId, ModuleInfo>,
    ) {
        for (&mid, module) in modules.iter_mut() {
            self.realize_module(tx, loop_handle, cfg, mid, module);
        }
    }

    fn __realize_module(
        &mut self,
        tx: &mpsc::Sender<Event>,
        loop_handle: &mut LoopHandle<SctkState>,
        cfg: &Config,
        mid: ModuleId,
        module: &mut ModuleInfo,
        opts: Option<ui::sctk::Options>,
    ) {
        if !module.toggled {
            return;
        }

        let opts_final = if let Some(o) = opts {
            o
        } else {
            let mut o = module.as_ref().manifest().options.clone();
            if let Some(value) = cfg.get(&module.name) {
                module
                    .as_mut()
                    .apply_config(&mut self.engine, value, &mut o);
            }
            o
        };

        let items = self.sctk.create_surfaces(opts_final);
        for CreatedSurface { sid, handles, size } in items {
            let tid = self.engine.attach_target(Arc::new(handles), size);
            self.by_module.entry(mid).or_default().push(tid);
            self.by_surface.insert(sid, (tid, mid));
            self.by_target.insert(tid, (sid, mid));
        }

        self.add_subscriptions(tx, loop_handle, &mid, module);
        for (key, factory) in module.as_ref().pipelines() {
            self.engine
                .register_pipeline(PipelineKey::Other(key), factory);
        }
    }

    fn realize_module(
        &mut self,
        tx: &mpsc::Sender<Event>,
        loop_handle: &mut LoopHandle<SctkState>,
        cfg: &Config,
        mid: ModuleId,
        module: &mut ModuleInfo,
    ) {
        self.__realize_module(tx, loop_handle, cfg, mid, module, None);
    }

    fn realize_module_with_opts(
        &mut self,
        tx: &mpsc::Sender<Event>,
        loop_handle: &mut LoopHandle<SctkState>,
        cfg: &Config,
        mid: ModuleId,
        module: &mut ModuleInfo,
        opts: ui::sctk::Options,
    ) {
        self.__realize_module(tx, loop_handle, cfg, mid, module, Some(opts));
    }

    fn unrealize_module(&mut self, loop_handle: &mut LoopHandle<SctkState>, mid: ModuleId) {
        let sids: Vec<SurfaceId> = self
            .by_surface
            .iter()
            .filter_map(|(sid, (_, owner))| if *owner == mid { Some(*sid) } else { None })
            .collect();
        let tids = self.by_module.remove(&mid).unwrap_or_default();

        for tid in tids {
            self.engine.detach_target(&tid);
        }
        self.by_surface.retain(|_, (_, owner)| *owner != mid);
        self.by_target.retain(|_, (_, owner)| *owner != mid);

        self.remove_subscriptions(loop_handle, mid);

        self.sctk.destroy_surfaces(&sids);
    }

    fn do_update(
        engine: &mut Engine,
        event: &orbit_api::Event<ErasedMsg>,
        (module, task): &mut (&mut ModuleInfo, &mut Option<UnraveledTask>),
        tid: &TargetId,
    ) -> bool {
        let (ut, redraw) = api_utils::unravel_task(module.as_mut().update(*tid, engine, event));
        **task = Some(ut);
        redraw
    }

    fn handle_task(
        mut utask: Option<UnraveledTask>,
        mid: &ModuleId,
        module: &ModuleInfo,
        tx: &mpsc::Sender<Event>,
        task_scheduler: &Scheduler<(ModuleId, ErasedMsg)>,
    ) {
        if let Some(ut) = utask.as_mut() {
            match ut.action() {
                api_utils::Action::ExitOrbit => {
                    _ = tx.send(Event::Dbus(DbusEvent::Exit));
                    return;
                }
                api_utils::Action::ExitModule => {
                    _ = tx.send(Event::Dbus(DbusEvent::Toggle(module.name.clone())));
                    return;
                }
                api_utils::Action::RedrawModule => {
                    _ = tx.send(Event::Ui(event::Ui::ForceRedraw(*mid)));
                }
                api_utils::Action::None => (),
            }

            if let Some(pending) = ut.tasks.take() {
                for task in pending {
                    let mid = *mid;
                    let fut = async move {
                        let msg = task.await.clone_for_send();
                        (mid, msg)
                    };
                    let _ = task_scheduler.schedule(fut);
                }
            }
        }
    }

    fn handle_sctk_event(
        engine: &mut Engine,
        modules: &mut HashMap<ModuleId, ModuleInfo>,
        event: &SctkEvent,
        mid: &ModuleId,
        tid: &TargetId,
        tx: &mpsc::Sender<Event>,
        task_scheduler: &Scheduler<(ModuleId, ErasedMsg)>,
    ) {
        if let Some(module) = modules.get_mut(mid) {
            let mut task = None;
            engine.handle_platform_event(
                tid,
                event,
                &mut Self::do_update,
                &mut (module, &mut task),
                tid,
            );
            Self::handle_task(task, mid, module, tx, task_scheduler);
        }
    }

    // TODO: only render those with changes
    #[allow(clippy::too_many_arguments)]
    fn render(
        engine: &mut Engine,
        modules: &mut HashMap<ModuleId, ModuleInfo>,
        sctk: &mut SctkApp,
        mid: &ModuleId,
        sid: &SurfaceId,
        tid: &TargetId,
        poll_override: bool,
        tx: &mpsc::Sender<Event>,
        task_scheduler: &Scheduler<(ModuleId, ErasedMsg)>,
    ) {
        if let Some(module) = modules.get_mut(mid) {
            if !sctk.state.surfaces.contains_key(sid) {
                return;
            }

            let mut task = None;
            let need = sctk
                .state
                .surfaces
                .get(sid)
                .map(|s| s.configured)
                .unwrap_or(false)
                && (poll_override
                    || engine.poll(tid, &mut Self::do_update, &mut (module, &mut task), tid));
            engine.render_if_needed(
                tid,
                need,
                &|tid, s: &ModuleInfo| s.as_ref().view(tid),
                module,
            );
            Self::handle_task(task, mid, module, tx, task_scheduler);
        }
    }

    fn format_commands(&self, module_name: &str) -> String {
        fn push_commands(reply: &mut String, module: &ModuleInfo, depth: usize) {
            if module.is_loaded() {
                let indent = "\t".repeat(depth);
                let sub_indent = "\t".repeat(depth + 1);

                writeln!(reply, "{}{}:", indent, module.name).unwrap();

                let commands = module.as_ref().manifest().commands;
                if commands.is_empty() {
                    writeln!(reply, "{}(no commands)", sub_indent).unwrap();
                    return;
                }

                for command in commands {
                    writeln!(reply, "{}{command}", sub_indent).unwrap();
                }
            }
        }

        let mut reply = String::new();

        if module_name.is_empty() {
            reply.push_str("Commands:\n");

            let mut modules = self.modules.iter().collect::<Vec<_>>();
            modules.sort_by(|a, b| a.1.name.cmp(&b.1.name));

            for (_, module) in modules {
                push_commands(&mut reply, module, 1);
            }
        } else if let Some((_, module)) = self
            .modules
            .iter()
            .find(|(_, module)| module.name == module_name)
        {
            push_commands(&mut reply, module, 0);
        } else {
            writeln!(reply, "Unknown module: {module_name}").unwrap();
        }

        reply
    }

    fn run(&mut self) {
        self.d_server.start();
        self.config_watcher.start(&self.config_path);

        let tx = self.tx.clone();
        let mut event_loop: EventLoop<SctkState> = EventLoop::try_new().expect("err");
        let _ = WaylandSource::new(self.sctk.conn.clone(), self.sctk.take_event_queue())
            .insert(event_loop.handle());

        let _ = event_loop.handle().insert_source(
            self.sctk_rx.take().expect("sctk_rx already taken"),
            |evt, _, _| {
                if let loop_channel::Event::Msg(e) = evt {
                    let _ = tx.send(Event::Ui(event::Ui::Sctk(e)));
                }
            },
        );
        let _ = event_loop.handle().insert_source(
            self.dbus_rx.take().expect("dbus_rx already taken"),
            |evt, _, _| {
                if let loop_channel::Event::Msg(e) = evt {
                    let _ = tx.send(Event::Dbus(e));
                }
            },
        );
        let _ = event_loop.handle().insert_source(
            self.config_rx.take().expect("config_rx already taken"),
            |evt, _, _| {
                if let loop_channel::Event::Msg(e) = evt {
                    let _ = tx.send(Event::Config(e));
                }
            },
        );

        let task_scheduler = {
            let tx = tx.clone();
            let (task_exec, task_scheduler) =
                executor::<(ModuleId, ErasedMsg)>().expect("create task executor");

            let _ = event_loop
                .handle()
                .insert_source(task_exec, move |(mid, msg), _, _| {
                    _ = tx.send(Event::Ui(event::Ui::Module(mid, SctkEvent::message(msg))));
                });

            task_scheduler
        };

        {
            let config = std::mem::take(&mut self.config);
            let mut modules = std::mem::take(&mut self.modules);
            self.realize_modules(&tx, &mut event_loop.handle(), &config, &mut modules);
            self.config = config;
            self.modules = modules;
        }

        let mut orbit_closed = false;
        while !orbit_closed {
            _ = event_loop.dispatch(None, &mut self.sctk.state);

            let mut need_tick = false;

            while let Ok(e) = self.rx.try_recv() {
                match e {
                    Event::Ui(ui_event) => {
                        need_tick = true;

                        match ui_event {
                            event::Ui::Orbit(msg) => match msg {
                                event::SctkMessage::SurfaceDestroyed(id) => {
                                    tracing::info!(id = id, "entered output removed");

                                    let sid = SurfaceId::new(id);

                                    self.sctk.state.remove_surface_by_surface_id(sid);

                                    if let Some((tid, _)) = self.by_surface.remove(&sid) {
                                        self.engine.detach_target(&tid);
                                        self.sctk.state.remove_surface_by_surface_id(sid);

                                        self.by_target.remove(&tid);
                                    }
                                }
                                event::SctkMessage::OutputCreated => {
                                    for (mid, module) in self
                                        .modules
                                        .iter()
                                        .filter(|(_, m)| m.is_loaded() && m.toggled)
                                    {
                                        let new_surfaces = self
                                            .sctk
                                            .ensure_surfaces(&module.as_ref().manifest().options);

                                        for (sid, target, size) in new_surfaces {
                                            let tid = self.engine.attach_target(target, size);

                                            self.by_module
                                                .entry(*mid)
                                                .and_modify(|ts| ts.push(tid));
                                            self.by_surface.insert(sid, (tid, *mid));
                                            self.by_target.insert(tid, (sid, *mid));
                                        }
                                    }
                                }
                            },
                            event::Ui::Sctk(sctk_event) => {
                                if let Some(sid) = sctk_event.surface_id() {
                                    if let Some((tid, mid)) = self.by_surface.get(&sid) {
                                        Self::handle_sctk_event(
                                            &mut self.engine,
                                            &mut self.modules,
                                            &sctk_event,
                                            mid,
                                            tid,
                                            &tx,
                                            &task_scheduler,
                                        );
                                    } else if !self.error_dialog.is_empty() {
                                        for (tid, _) in
                                            self.error_dialog.iter().filter(|(_, s)| *s == sid)
                                        {
                                            self.engine.handle_platform_event(
                                                tid,
                                                &sctk_event,
                                                &mut |_, _, _, _| false,
                                                &mut (),
                                                &(),
                                            );
                                        }
                                    }
                                } else {
                                    for (_, (tid, mid)) in self.by_surface.iter() {
                                        Self::handle_sctk_event(
                                            &mut self.engine,
                                            &mut self.modules,
                                            &sctk_event,
                                            mid,
                                            tid,
                                            &tx,
                                            &task_scheduler,
                                        );
                                    }
                                }
                            }
                            event::Ui::Module(mid, sctk_event) => {
                                // TODO: is erased message even necessary here?
                                if let Some(base) = sctk::take_erased_from_message(&sctk_event) {
                                    for tid in self.by_module.get(&mid).into_iter().flatten() {
                                        let event =
                                            ui::sctk::SctkEvent::message(base.clone_for_send());
                                        Self::handle_sctk_event(
                                            &mut self.engine,
                                            &mut self.modules,
                                            &event,
                                            &mid,
                                            tid,
                                            &tx,
                                            &task_scheduler,
                                        );
                                    }
                                } else {
                                    for tid in self.by_module.get(&mid).into_iter().flatten() {
                                        Self::handle_sctk_event(
                                            &mut self.engine,
                                            &mut self.modules,
                                            &sctk_event,
                                            &mid,
                                            tid,
                                            &tx,
                                            &task_scheduler,
                                        );
                                    }
                                }
                            }
                            event::Ui::ForceRedraw(mid) => {
                                let Some(tids) = self.by_module.get(&mid) else {
                                    continue;
                                };
                                for (tid, sid) in tids
                                    .iter()
                                    .filter_map(|t| self.by_target.get(t).map(|(s, _)| (t, s)))
                                {
                                    Self::render(
                                        &mut self.engine,
                                        &mut self.modules,
                                        &mut self.sctk,
                                        &mid,
                                        sid,
                                        tid,
                                        true,
                                        &tx,
                                        &task_scheduler,
                                    );
                                }
                            }
                        }
                    }
                    Event::Dbus(dbus_event) => match dbus_event {
                        DbusEvent::Reload(resp_tx) => {
                            for module in self.modules.values_mut() {
                                if module.is_loaded() {
                                    module.as_mut().cleanup(&mut self.engine);
                                }
                            }

                            let all_sids: Vec<_> = self.by_surface.keys().copied().collect();
                            self.by_surface.clear();
                            self.sctk.destroy_surfaces(&all_sids);
                            for (_mid, tids) in self.by_module.drain() {
                                for tid in tids {
                                    self.engine.detach_target(&tid);
                                }
                            }

                            let resp = match Self::discover_and_load_modules(
                                &mut self.config,
                                &self.config_path,
                                &mut self.engine,
                                Some(self.modules.len()),
                            ) {
                                Ok(mut modules) => {
                                    let config = std::mem::take(&mut self.config);
                                    self.realize_modules(
                                        &tx,
                                        &mut event_loop.handle(),
                                        &config,
                                        &mut modules,
                                    );
                                    self.config = config;
                                    self.modules = modules;
                                    "Reloaded".into()
                                }
                                Err(e) => e,
                            };

                            let _ = resp_tx.send(resp);
                        }
                        DbusEvent::Modules(resp_tx) => {
                            let mut reply = String::with_capacity(128 + self.modules.len() * 32);
                            reply.push_str("Loaded modules:\n");

                            for (_id, module) in self.modules.iter() {
                                let loaded = if module.is_loaded() {
                                    "loaded"
                                } else {
                                    "unloaded"
                                };
                                let shown = if module.toggled { ", shown" } else { "" };
                                writeln!(reply, "\t{} ({}{})", module.name, loaded, shown).unwrap();
                            }

                            let _ = resp_tx.send(reply);
                        }
                        DbusEvent::Commands(module_name, resp_tx) => {
                            let _ = resp_tx.send(self.format_commands(&module_name));
                        }
                        DbusEvent::Toggle(module_name) => {
                            let Some(&mid) = self
                                .modules
                                .iter()
                                .find(|(_, m)| m.name == module_name && m.is_loaded())
                                .map(|(mid, _)| mid)
                            else {
                                continue;
                            };

                            let mut module = self.modules.remove(&mid).expect("just found");
                            if module.toggled {
                                module.toggled = false;
                                self.unrealize_module(&mut event_loop.handle(), mid);
                            } else {
                                let config = std::mem::take(&mut self.config);
                                module.toggled = true;
                                self.realize_module(
                                    &tx,
                                    &mut event_loop.handle(),
                                    &config,
                                    mid,
                                    &mut module,
                                );
                                self.config = config;
                            }
                            self.modules.insert(mid, module);
                        }
                        DbusEvent::Command(module_name, command_name) => {
                            let Some((&mid, module)) = self
                                .modules
                                .iter()
                                .find(|(_, m)| m.name == module_name && m.is_loaded())
                            else {
                                tracing::warn!(module = %module_name, command = %command_name, "command for unknown module");
                                continue;
                            };

                            let Some(message) = module.as_ref().command_message(&command_name)
                            else {
                                tracing::warn!(module = %module_name, command = %command_name, "unknown module command");
                                continue;
                            };

                            let _ = tx.send(Event::Ui(event::Ui::Module(
                                mid,
                                SctkEvent::message(message),
                            )));
                        }
                        DbusEvent::Exit => {
                            orbit_closed = true;
                            event_loop.get_signal().stop();
                        }
                    },
                    Event::Config(config_event) => match config_event {
                        ConfigEvent::Reload(new_config) => {
                            if self.config == new_config && self.errors.is_empty() {
                                continue;
                            }

                            let mut errors = Vec::new();
                            let instructions = match compare_configs(&self.config, &new_config) {
                                Ok(i) => i,
                                Err(e) => {
                                    let _ =
                                        tx.send(Event::Config(ConfigEvent::Err(vec![e.into()])));
                                    continue;
                                }
                            };
                            let mid_by_name: HashMap<String, ModuleId> = self
                                .modules
                                .iter()
                                .map(|(&k, v)| (v.name.clone(), k))
                                .collect();
                            let mut modules = std::mem::take(&mut self.modules);
                            for (
                                name,
                                ConfigInstruction {
                                    should_unrealize,
                                    should_realize,
                                    config_changed,
                                },
                            ) in instructions
                            {
                                let Some((&mid, module)) =
                                    mid_by_name.get(name.as_str()).and_then(|mid| {
                                        let module = modules.get_mut(mid)?;
                                        Some((mid, module))
                                    })
                                else {
                                    errors.push(format!("Module {} could not be found.", &name));
                                    continue;
                                };

                                if should_unrealize {
                                    module.toggled = false;
                                    self.unrealize_module(&mut event_loop.handle(), mid);
                                    module.unload(&mut self.engine);
                                }

                                if (should_realize || config_changed)
                                    && let Err(e) = Self::load_module(
                                        &mut self.engine,
                                        new_config.get(&name),
                                        module,
                                    )
                                {
                                    errors.push(e);
                                    continue;
                                }

                                if should_realize {
                                    module.toggled = module.as_ref().manifest().show_on_startup;
                                    if module.toggled {
                                        self.realize_module(
                                            &tx,
                                            &mut event_loop.handle(),
                                            &new_config,
                                            mid,
                                            module,
                                        );
                                    }
                                }

                                if !should_realize
                                    && config_changed
                                    && new_config.enabled(&name)
                                    && let Some(cfg) = new_config.get(&module.name)
                                {
                                    let mut opts = module.as_ref().manifest().options.clone();
                                    let must_rebuild = module.as_mut().apply_config(
                                        &mut self.engine,
                                        cfg,
                                        &mut opts,
                                    );

                                    if must_rebuild {
                                        self.unrealize_module(&mut event_loop.handle(), mid);
                                        self.realize_module_with_opts(
                                            &tx,
                                            &mut event_loop.handle(),
                                            &new_config,
                                            mid,
                                            module,
                                            opts,
                                        );
                                    } else {
                                        self.remove_subscriptions(&mut event_loop.handle(), mid);
                                        self.add_subscriptions(
                                            &tx,
                                            &mut event_loop.handle(),
                                            &mid,
                                            module,
                                        );
                                        let _ = tx.send(Event::Ui(event::Ui::ForceRedraw(mid)));
                                    }
                                }
                            }
                            self.modules = modules;

                            if errors.is_empty() {
                                self.config = new_config;
                                self.hide_error();
                            } else if !errors.is_empty() {
                                let _ = tx.send(Event::Config(ConfigEvent::Err(errors)));
                            }
                        }
                        ConfigEvent::Err(errors) => {
                            tracing::warn!(?errors, "config errors");
                            self.show_error(errors);
                        }
                    },
                }
            }

            if need_tick {
                for (mid, tids) in self.by_module.iter() {
                    for (tid, sid) in tids
                        .iter()
                        .filter_map(|t| self.by_target.get(t).map(|(s, _)| (t, s)))
                    {
                        Self::render(
                            &mut self.engine,
                            &mut self.modules,
                            &mut self.sctk,
                            mid,
                            sid,
                            tid,
                            false,
                            &tx,
                            &task_scheduler,
                        );
                    }
                }
                for (tid, _) in self.error_dialog.iter() {
                    let need = self.engine.poll(
                        tid,
                        &mut |_, _: &ui::event::Event<ErasedMsg, SctkEvent>, (), _| false,
                        &mut (),
                        &(),
                    );
                    self.engine
                        .render_if_needed(tid, need, &dialog::error_view, &mut self.errors);
                }
            }
        }

        self.d_server.stop();
        self.config_watcher.stop();
    }
}

pub fn main() {
    // TODO: get config_path from args

    trace::init();
    tracing::info!("orbitd starting");

    let mut orbit = Orbit::new(None).expect("woops");
    orbit.run();

    tracing::info!("orbitd stopped");
}
