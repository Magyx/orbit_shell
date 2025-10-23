// TODO: better error messages cmon dude
use std::{
    collections::HashMap,
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use calloop::{EventLoop, channel as loop_channel};
use serde_yml::{Mapping, Value};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use ui::{
    graphics::{Engine, TargetId},
    render::pipeline::PipelineKey,
    sctk::{SctkEvent, SurfaceId, state::SctkState},
};

use orbit_api::{ErasedMsg, OrbitLoop, runtime::OrbitModuleDyn};
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
mod event;
mod module;
mod sctk;

struct ModuleInfo {
    inner: Module,
    toggled: bool,
}

impl ModuleInfo {
    pub fn new(module: Module) -> Self {
        let toggled = module.as_ref().manifest().show_on_startup;
        Self {
            inner: module,
            toggled,
        }
    }

    pub fn as_ref(&self) -> &dyn OrbitModuleDyn {
        self.inner.as_ref()
    }

    pub fn as_mut(&mut self) -> &mut dyn OrbitModuleDyn {
        self.inner.as_mut()
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

    engine: Engine<'a, ErasedMsg>,
    by_module: HashMap<ModuleId, Vec<TargetId>>,
    by_surface: HashMap<SurfaceId, (TargetId, ModuleId)>,
}

impl<'a> Orbit<'a> {
    fn new(config_path: Option<PathBuf>) -> Result<Self, String> {
        let (sctk_rx, sctk) = SctkApp::new()?;
        let mut engine = Engine::default();

        let config_path = config_path.unwrap_or_else(config::xdg_config_home);
        config::ensure_exists(&config_path)?;
        let mut config = config::load_cfg(&config_path).map_err(|e| e.to_string())?;
        let modules = Self::discover_modules(&mut engine, &mut config, &config_path, None)?;
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

            engine,
            by_module: HashMap::with_capacity(modules_len),
            by_surface: HashMap::with_capacity(modules_len),
        })
    }

    fn discover_modules(
        engine: &mut Engine<'a, ErasedMsg>,
        cfg: &mut serde_yml::Value,
        config_path: &Path,
        prev_module_len: Option<usize>,
    ) -> Result<HashMap<ModuleId, ModuleInfo>, String> {
        let mods_dir = config::modules_dir(config_path);
        let prev_cfg = cfg.clone();

        if cfg.get("modules").is_none() {
            cfg["modules"] = Value::Mapping(Mapping::new());
        }
        let map = cfg.as_mapping_mut().unwrap();

        let mut modules = HashMap::with_capacity(prev_module_len.unwrap_or_default());
        let mut next_id: u32 = 0;
        for entry in fs::read_dir(&mods_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().map(|e| e == "so").unwrap_or(false) {
                let module = Module::new(&path)?;
                let name = module.as_ref().manifest().name;
                let name_val = Value::from(name);

                let enabled: bool = {
                    let modules_val = map.get_mut("modules").expect("mapping just created");
                    let modules_map = modules_val
                        .as_mapping_mut()
                        .expect("\"modules\" must be a mapping");

                    if !modules_map.contains_key(&name_val) {
                        modules_map.insert(name_val.clone(), Value::Bool(false));
                        false
                    } else {
                        modules_map
                            .get(&name_val)
                            .and_then(Value::as_bool)
                            .ok_or_else(|| format!("Module value for {} is not a bool!", name))?
                    }
                };

                if enabled {
                    if !map.contains_key(&name_val) {
                        map.insert(name_val.clone(), Value::Mapping(Mapping::new()));
                    }

                    {
                        let cfg_for_module = map.get_mut(&name_val).expect("inserted above");
                        module.as_ref().init_config(cfg_for_module);
                    }
                    {
                        let cfg_for_module = map.get(&name_val).expect("inserted above");
                        module.as_ref().validate_config(cfg_for_module)?;
                    }

                    for (key, factory) in module.as_ref().pipelines() {
                        engine.register_pipeline(PipelineKey::Other(key), factory);
                    }
                }

                let id = ModuleId(next_id);
                next_id = next_id.wrapping_add(1);
                modules.insert(id, ModuleInfo::new(module));
            }
        }

        if prev_cfg != *cfg {
            config::store_cfg(config_path, cfg).map_err(|e| e.to_string())?;
        }

        Ok(modules)
    }

    fn realize_loaded_modules(&mut self) {
        for (&mid, module) in self.modules.iter_mut() {
            if module.toggled {
                let opts = module.as_ref().manifest().options.clone();
                let items = self.sctk.create_surfaces(opts);

                for CreatedSurface { sid, handles, size } in items {
                    let tid = self.engine.attach_target(Arc::new(handles), size);
                    self.by_module.entry(mid).or_default().push(tid);
                    self.by_surface.insert(sid, (tid, mid));
                }

                let module_name = module.as_ref().manifest().name;
                module.as_mut().config_updated(
                    &mut self.engine,
                    self.config.get(module_name).expect("name needs to exist"),
                );
            }

            for (key, factory) in module.as_ref().pipelines() {
                self.engine
                    .register_pipeline(PipelineKey::Other(key), factory);
            }
        }
    }

    fn realize_module(&mut self, mid: ModuleId, module: &mut ModuleInfo) {
        if module.toggled {
            let opts = module.as_ref().manifest().options.clone();
            let items = self.sctk.create_surfaces(opts);

            for CreatedSurface { sid, handles, size } in items {
                let tid = self.engine.attach_target(Arc::new(handles), size);
                self.by_module.entry(mid).or_default().push(tid);
                self.by_surface.insert(sid, (tid, mid));
            }

            let module_name = module.as_ref().manifest().name;
            module.as_mut().config_updated(
                &mut self.engine,
                self.config.get(module_name).expect("name needs to exist"),
            );
        }

        for (key, factory) in module.as_ref().pipelines() {
            self.engine
                .register_pipeline(PipelineKey::Other(key), factory);
        }
    }

    fn unrealize_module(&mut self, mid: ModuleId) {
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

        self.sctk.destroy_surfaces(&sids);
    }

    fn tick_all_targets(&mut self, orbit: &orbit_api::OrbitLoop) {
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
    }

    fn run(mut self) {
        self.d_server.start();
        self.config_watcher.start(&self.config_path);

        self.realize_loaded_modules();

        let (tx, rx) = mpsc::channel::<Event>();
        let orbit_loop = OrbitLoop::new();
        let mut event_loop: EventLoop<SctkState> = EventLoop::try_new().expect("err");
        let _ = WaylandSource::new(self.sctk.conn.clone(), self.sctk.take_event_queu())
            .insert(event_loop.handle());

        let _ = event_loop.handle().insert_source(
            self.sctk_rx.take().expect("sctk_rx already taken"),
            |evt, _meta, _state| {
                if let loop_channel::Event::Msg(e) = evt {
                    let _ = tx.send(Event::Sctk(e));
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

        while !orbit_loop.should_close() {
            _ = event_loop.dispatch(None, &mut self.sctk.state);

            while let Ok(e) = rx.try_recv() {
                match e {
                    Event::Sctk(sctk_event) => {
                        let mut handle = |tid: &TargetId, mid: &ModuleId| {
                            if let Some(module) = self.modules.get_mut(mid) {
                                self.engine.handle_platform_event(
                                    tid,
                                    &sctk_event,
                                    &mut |eng, e, s: &mut ModuleInfo, ctl| {
                                        s.as_mut().update(*tid, eng, e, ctl)
                                    },
                                    module,
                                    &orbit_loop,
                                );
                            }
                        };
                        match sctk_event.surface_id() {
                            Some(sid) => {
                                if let Some((tid, mid)) = self.by_surface.get(&sid) {
                                    handle(tid, mid);
                                }
                            }
                            None => {
                                for (_, (tid, mid)) in self.by_surface.iter() {
                                    handle(tid, mid);
                                }
                            }
                        }
                    }
                    Event::Dbus(dbus_event) => match dbus_event {
                        DbusEvent::Reload(resp_tx) => {
                            for module in self.modules.values_mut() {
                                module.as_mut().cleanup(&mut self.engine);
                            }

                            let all_sids: Vec<_> = self.by_surface.keys().copied().collect();
                            self.by_surface.clear();
                            self.sctk.destroy_surfaces(&all_sids);

                            for (_mid, tids) in self.by_module.drain() {
                                for tid in tids {
                                    self.engine.detach_target(&tid);
                                }
                            }

                            let resp = match Self::discover_modules(
                                &mut self.engine,
                                &mut self.config,
                                &self.config_path,
                                Some(self.modules.len()),
                            ) {
                                Ok(mods) => {
                                    self.modules = mods;
                                    self.realize_loaded_modules();
                                    "Reloaded".into()
                                }
                                Err(e) => e,
                            };

                            let _ = resp_tx.send(resp);
                        }
                        DbusEvent::Modules(resp_tx) => {
                            let mut reply = String::with_capacity(128 + self.modules.len() * 24);
                            reply.push_str("Loaded modules:\n");

                            for (_id, module) in self.modules.iter() {
                                let name = module.as_ref().manifest().name;
                                writeln!(reply, "\t{}", name).unwrap();
                            }

                            let _ = resp_tx.send(reply);
                        }
                        DbusEvent::Toggle(module_name) => {
                            let Some(&mid) = self
                                .modules
                                .iter()
                                .find(|(_, m)| m.as_ref().manifest().name == module_name)
                                .map(|(mid, _)| mid)
                            else {
                                continue;
                            };
                            let mut module = self.modules.remove(&mid).expect("just found");

                            if module.toggled {
                                self.unrealize_module(mid);
                                module.toggled = false;
                            } else {
                                self.realize_module(mid, &mut module);
                                module.toggled = true;
                            }

                            self.modules.insert(mid, module);
                        }
                        DbusEvent::Exit => {
                            todo!()
                        }
                    },
                    Event::Config(config_event) => {
                        match config_event {
                            ConfigEvent::Reload(value) => {
                                if self.config == value {
                                    continue;
                                }

                                self.config = value;
                                let modules_map =
                                    self.config.as_mapping_mut().expect("mapping just created");

                                let mut errors = Vec::new();
                                for module in self.modules.values_mut() {
                                    let name = module.as_ref().manifest().name;
                                    let value = &modules_map[name];
                                    if let Err(e) = module.as_ref().validate_config(value) {
                                        errors.push((name.to_string(), e));
                                    } else {
                                        module.as_mut().config_updated(&mut self.engine, value);
                                    }
                                }

                                // TODO: show the dialog when error
                                dbg!(errors);
                            }
                            ConfigEvent::Err(e) => {
                                // TODO: show the dialog when error
                                dbg!(e);
                            }
                        }
                    }
                }
            }

            self.tick_all_targets(&orbit_loop);
        }

        self.d_server.stop();
        self.config_watcher.stop();
    }
}

pub fn main() {
    // TODO: get config_path from args

    let orbit = Orbit::new(None).expect("woops");
    orbit.run();
}
