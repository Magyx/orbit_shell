// TODO: better error messages cmon dude
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use calloop::{
    EventLoop, LoopHandle, RegistrationToken, channel as loop_channel,
    timer::{TimeoutAction, Timer},
};
use serde_yml::{Mapping, Value};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use ui::{
    graphics::{Engine, TargetId},
    render::pipeline::PipelineKey,
    sctk::{SctkEvent, SurfaceId, state::SctkState},
};

use orbit_api::{ErasedMsg, OrbitCtl, runtime::OrbitModuleDyn};
use orbit_dbus::DbusEvent;

use crate::{
    config::{ConfigEvent, ConfigWatcher},
    sctk::{CreatedSurface, SctkApp},
};

use {
    dbus::OrbitdServer,
    event::Event,
    module::{Module, ModuleId},
};

mod config;
mod dbus;
mod dialog;
mod event;
mod module;
mod sctk;
mod trace;

struct ConfigInstruction {
    should_unrealize: bool,
    should_realize: bool,
    config_changed: bool,
}

struct ModuleInfo {
    name: String,
    path: PathBuf,
    inner: Option<Module>,
    toggled: bool,
}

impl ModuleInfo {
    pub fn new(name: String, path: PathBuf) -> Self {
        Self {
            name,
            path,
            inner: None,
            toggled: false,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.inner.is_some()
    }

    pub fn ensure_loaded(&mut self) -> Result<(), String> {
        if self.inner.is_none() {
            self.inner = Some(Module::new(&self.path)?);
        }

        Ok(())
    }

    pub fn unload(&mut self, engine: &mut Engine<'_, ErasedMsg>) {
        if let Some(mut module) = self.inner.take() {
            module.as_mut().cleanup(engine);
        }
        self.toggled = false;
    }

    pub fn as_ref(&self) -> &dyn OrbitModuleDyn {
        self.inner.as_ref().expect("module not loaded").as_ref()
    }

    pub fn as_mut(&mut self) -> &mut dyn OrbitModuleDyn {
        self.inner.as_mut().expect("module not loaded").as_mut()
    }
}

struct Orbit<'a> {
    dbus_rx: Option<loop_channel::Channel<DbusEvent>>,
    d_server: OrbitdServer,

    config_rx: Option<loop_channel::Channel<ConfigEvent>>,
    config_watcher: ConfigWatcher,

    sctk_rx: Option<loop_channel::Channel<SctkEvent>>,
    sctk: SctkApp,

    config_path: PathBuf,
    config: serde_yml::Value,
    modules: HashMap<ModuleId, ModuleInfo>,
    subs: HashMap<ModuleId, Vec<RegistrationToken>>,
    error_dialog: Vec<(TargetId, SurfaceId)>,
    errors: Vec<String>,

    engine: Engine<'a, ErasedMsg>,
    by_module: HashMap<ModuleId, Vec<TargetId>>,
    by_surface: HashMap<SurfaceId, (TargetId, ModuleId)>,
}

impl<'a> Orbit<'a> {
    fn new(config_path: Option<PathBuf>) -> Result<Self, String> {
        let config_path = config_path.unwrap_or_else(config::xdg_config_home);

        let (sctk_rx, sctk) = SctkApp::new()?;
        let mut engine = Engine::default();

        let mut config = config::load_cfg(&config_path).map_err(|e| e.to_string())?;
        let modules =
            Self::discover_and_load_modules(&mut config, &config_path, &mut engine, None)?;
        let modules_len = modules.len();

        let (dbus_rx, d_server) = OrbitdServer::new();
        let (config_rx, config_watcher) = ConfigWatcher::new();

        Ok(Self {
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
        })
    }

    fn is_enabled(map: Option<&serde_yml::Mapping>, name: &str) -> Result<bool, String> {
        let Some(map) = map else {
            return Ok(false);
        };
        let Some(modules_val) = map.get("modules") else {
            return Ok(false);
        };
        let Some(modules_map) = modules_val.as_mapping() else {
            return Ok(false);
        };
        let Some(module) = modules_map.get(name) else {
            return Ok(false);
        };
        module
            .as_bool()
            .ok_or_else(|| format!("Module value for {} is not a bool!", name))
    }

    fn discover_and_load_modules(
        cfg: &mut serde_yml::Value,
        config_path: &Path,
        engine: &mut Engine<'a, ErasedMsg>,
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
        cfg: &mut serde_yml::Value,
        engine: &mut Engine<'a, ErasedMsg>,
    ) -> Result<HashMap<ModuleId, ModuleInfo>, String> {
        let mut loaded_modules = HashMap::with_capacity(modules.len());
        let mut next_id: u32 = 0;
        for mut module in modules {
            let enabled = Self::is_enabled(cfg.as_mapping(), &module.name)?;
            if enabled {
                Self::load_module(engine, cfg.as_mapping_mut().unwrap(), &mut module)?;
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
        engine: &mut Engine<'a, ErasedMsg>,
        map: &mut serde_yml::Mapping,
        module: &mut ModuleInfo,
    ) -> Result<(), String> {
        let cfg_for_module = map
            .entry(Value::String(module.name.clone()))
            .or_insert_with(|| Value::Mapping(Mapping::new()));

        module.ensure_loaded()?;
        module.as_ref().validate_config(cfg_for_module)?;

        for (key, factory) in module.as_ref().pipelines() {
            engine.register_pipeline(PipelineKey::Other(key), factory);
        }

        Ok(())
    }

    pub fn compare_configs(
        old_config: &Value,
        new_config: &Value,
    ) -> Result<HashMap<String, ConfigInstruction>, &'static str> {
        fn to_bool_map(m: &Mapping) -> Result<HashMap<String, bool>, &'static str> {
            m.iter()
                .map(|(k, v)| {
                    let key = k.as_str().ok_or("Key is not a string")?.to_owned();
                    let val = v.as_bool().ok_or("Value of module is not a bool")?;
                    Ok((key, val))
                })
                .collect::<Result<HashMap<_, _>, _>>()
        }

        let (old_root, new_root) = match (old_config.as_mapping(), new_config.as_mapping()) {
            (Some(o), Some(n)) => (o, n),
            _ => return Err("Config could not be parsed!"),
        };

        let Some(old_modules_map) = old_root.get("modules").and_then(Value::as_mapping) else {
            return Ok(HashMap::new());
        };
        let Some(new_modules_map) = new_root.get("modules").and_then(Value::as_mapping) else {
            return Ok(HashMap::new());
        };

        let old_modules = to_bool_map(old_modules_map)?;
        let new_modules = to_bool_map(new_modules_map)?;

        let mut module_names: HashSet<String> = old_modules.keys().cloned().collect();
        module_names.extend(new_modules.keys().cloned());

        let mut out: HashMap<String, ConfigInstruction> = HashMap::new();
        for name in module_names {
            let old_enabled = *old_modules.get(&name).unwrap_or(&false);
            let new_enabled = *new_modules.get(&name).unwrap_or(&false);

            let should_unrealize = old_enabled && !new_enabled;
            let should_realize = !old_enabled && new_enabled;

            let old_cfg = old_root.get(name.as_str()).and_then(Value::as_mapping);
            let new_cfg = new_root.get(name.as_str()).and_then(Value::as_mapping);
            let config_changed = old_cfg != new_cfg;

            out.insert(
                name,
                ConfigInstruction {
                    should_unrealize,
                    should_realize,
                    config_changed,
                },
            );
        }

        Ok(out)
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
                None => {}
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
                                let _ = ui_tx.send(Event::Ui(event::Ui::Orbit(
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
                                let _ = ui_tx.send(Event::Ui(event::Ui::Orbit(
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
            self.subs.insert(*mid, tokens);
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
        cfg: &mut serde_yml::Value,
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
        cfg: &serde_yml::Value,
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
            if let Some(cfg) = cfg.get(&module.name) {
                module.as_mut().apply_config(&mut self.engine, cfg, &mut o);
            }
            o
        };

        let items = self.sctk.create_surfaces(opts_final);
        for CreatedSurface { sid, handles, size } in items {
            let tid = self.engine.attach_target(Arc::new(handles), size);
            self.by_module.entry(mid).or_default().push(tid);
            self.by_surface.insert(sid, (tid, mid));
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
        cfg: &serde_yml::Value,
        mid: ModuleId,
        module: &mut ModuleInfo,
    ) {
        self.__realize_module(tx, loop_handle, cfg, mid, module, None);
    }

    fn realize_module_with_opts(
        &mut self,
        tx: &mpsc::Sender<Event>,
        loop_handle: &mut LoopHandle<SctkState>,
        cfg: &serde_yml::Value,
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

        self.remove_subscriptions(loop_handle, mid);

        self.sctk.destroy_surfaces(&sids);
    }

    fn tick_all_targets(&mut self, orbit: &orbit_api::OrbitCtl) {
        for (&mid, tids) in self.by_module.clone().iter() {
            for &tid in tids {
                if let Some(module) = self.modules.get_mut(&mid) {
                    let need = self.engine.poll(
                        &tid,
                        &mut |eng, e, s: &mut ModuleInfo, ctl| s.as_mut().update(tid, eng, e, ctl),
                        module,
                        orbit,
                    );
                    self.engine.render_if_needed(
                        &tid,
                        need,
                        &|tid, s: &ModuleInfo| s.as_ref().view(tid),
                        module,
                    );
                }
            }
        }
        for (tid, _) in self.error_dialog.iter() {
            let need = self.engine.poll(
                tid,
                &mut |_, _: &ui::event::Event<ErasedMsg, SctkEvent>, (), _| false,
                &mut (),
                orbit,
            );
            self.engine
                .render_if_needed(tid, need, &dialog::error_view, &mut self.errors);
        }
    }

    fn run(&mut self) {
        self.d_server.start();
        self.config_watcher.start(&self.config_path);

        let (tx, rx) = mpsc::channel::<Event>();
        let orbit_loop = OrbitCtl::new();
        let mut event_loop: EventLoop<SctkState> = EventLoop::try_new().expect("err");
        let _ = WaylandSource::new(self.sctk.conn.clone(), self.sctk.take_event_queu())
            .insert(event_loop.handle());

        let _ = event_loop.handle().insert_source(
            self.sctk_rx.take().expect("sctk_rx already taken"),
            |evt, _meta, _state| {
                if let loop_channel::Event::Msg(e) = evt {
                    let _ = tx.send(Event::Ui(event::Ui::Sctk(e)));
                }
            },
        );
        let _ = event_loop.handle().insert_source(
            self.dbus_rx.take().expect("dbus_rx already taken"),
            |evt, _meta, _state| {
                if let loop_channel::Event::Msg(e) = evt {
                    let _ = tx.send(Event::Dbus(e));
                }
            },
        );
        let _ = event_loop.handle().insert_source(
            self.config_rx.take().expect("config_rx already taken"),
            |evt, _meta, _state| {
                if let loop_channel::Event::Msg(e) = evt {
                    let _ = tx.send(Event::Config(e));
                }
            },
        );

        {
            let mut config = std::mem::take(&mut self.config);
            let mut modules = std::mem::take(&mut self.modules);
            self.realize_modules(&tx, &mut event_loop.handle(), &mut config, &mut modules);
            self.config = config;
            self.modules = modules;
        }

        while !orbit_loop.orbit_should_close() {
            _ = event_loop.dispatch(None, &mut self.sctk.state);

            let mut need_tick = false;

            while let Ok(e) = rx.try_recv() {
                match e {
                    Event::Ui(ui_event) => {
                        need_tick = true;

                        let mut handle_for = |tid: &TargetId, mid: &ModuleId, event: &SctkEvent| {
                            if let Some(module) = self.modules.get_mut(mid) {
                                self.engine.handle_platform_event(
                                    tid,
                                    event,
                                    &mut |eng, e, s: &mut ModuleInfo, ctl| {
                                        s.as_mut().update(*tid, eng, e, ctl)
                                    },
                                    module,
                                    &orbit_loop,
                                );
                            }
                        };
                        match ui_event {
                            event::Ui::Sctk(sctk_event) => match sctk_event.surface_id() {
                                Some(sid) => {
                                    if let Some((tid, mid)) = self.by_surface.get(&sid) {
                                        handle_for(tid, mid, &sctk_event);
                                    } else if !self.error_dialog.is_empty() {
                                        for (tid, _) in
                                            self.error_dialog.iter().filter(|(_, s)| *s == sid)
                                        {
                                            self.engine.handle_platform_event(
                                                tid,
                                                &sctk_event,
                                                &mut |_, _, _, _| false,
                                                &mut (),
                                                &orbit_loop,
                                            );
                                        }
                                    }
                                }
                                None => {
                                    for (_, (tid, mid)) in self.by_surface.iter() {
                                        handle_for(tid, mid, &sctk_event);
                                    }
                                }
                            },
                            event::Ui::Orbit(mid, sctk_event) => {
                                if let Some(base) = sctk::take_erased_from_message(&sctk_event) {
                                    for tid in self.by_module.get(&mid).into_iter().flatten() {
                                        let event =
                                            ui::sctk::SctkEvent::message(base.clone_for_send());
                                        handle_for(tid, &mid, &event);
                                    }
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
                                    let mut config = std::mem::take(&mut self.config);
                                    self.realize_modules(
                                        &tx,
                                        &mut event_loop.handle(),
                                        &mut config,
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
                        DbusEvent::Exit => {
                            orbit_loop.close_orbit();
                            event_loop.get_signal().stop();
                        }
                    },
                    Event::Config(config_event) => match config_event {
                        ConfigEvent::Reload(mut new_config) => {
                            if self.config == new_config {
                                continue;
                            }

                            let mut errors = Vec::new();
                            let instructions =
                                match Self::compare_configs(&self.config, &new_config) {
                                    Ok(i) => i,
                                    Err(e) => {
                                        let _ = tx
                                            .send(Event::Config(ConfigEvent::Err(vec![e.into()])));
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

                                let new_enabled = new_config
                                    .as_mapping()
                                    .map(|m| Self::is_enabled(Some(m), &name).unwrap_or(false))
                                    .unwrap_or(false);

                                if should_unrealize {
                                    module.toggled = false;
                                    self.unrealize_module(&mut event_loop.handle(), mid);
                                    module.unload(&mut self.engine);
                                }

                                if (should_realize || config_changed) && new_enabled {
                                    let new_config_map = new_config
                                        .as_mapping_mut()
                                        .expect("should exist from compare_configs");
                                    if let Err(e) =
                                        Self::load_module(&mut self.engine, new_config_map, module)
                                    {
                                        errors.push(e);
                                        continue;
                                    }
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
                                    && new_enabled
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
                                        let _ = tx.send(Event::Ui(event::Ui::Orbit(
                                            mid,
                                            SctkEvent::Redraw,
                                        )));
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
                self.tick_all_targets(&orbit_loop);
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
