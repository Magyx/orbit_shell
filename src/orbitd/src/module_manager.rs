use std::thread::JoinHandle;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use calloop::{LoopHandle, RegistrationToken, channel as loop_channel};
use orbit_api::{Engine, ErasedMsg};
use orbit_dbus::DbusEvent;
use ui::{
    graphics::TargetId,
    render::pipeline::PipelineKey,
    sctk::{SctkEvent, SurfaceId, state::SctkState},
};

use crate::event::RuntimeSender;
use crate::{
    api_utils::{self, UnraveledTask},
    config::{self, Config},
    event::{self, Event},
    module::{ModuleId, ModuleInfo},
    sctk::{CreatedSurface, SctkApp},
    subscriptions::StreamHandle,
};

pub struct ModuleManager {
    modules: HashMap<ModuleId, ModuleInfo>,
    sub_tokens: HashMap<ModuleId, Vec<RegistrationToken>>,
    dispatch_tokens: HashMap<ModuleId, Vec<StreamHandle>>,
    by_module: HashMap<ModuleId, Vec<TargetId>>,
    by_surface: HashMap<SurfaceId, (TargetId, ModuleId)>,
    by_target: HashMap<TargetId, (SurfaceId, ModuleId)>,

    pending_threads: Vec<JoinHandle<()>>,
}

impl ModuleManager {
    pub fn new(
        config: &mut Config,
        config_path: &Path,
        engine: &mut Engine<'_>,
    ) -> Result<Self, String> {
        let modules = Self::discover_and_load_modules(config, config_path, engine, None)?;
        let modules_len = modules.len();

        Ok(Self {
            modules,
            sub_tokens: HashMap::new(),
            dispatch_tokens: HashMap::new(),
            by_module: HashMap::with_capacity(modules_len),
            by_surface: HashMap::with_capacity(modules_len),
            by_target: HashMap::with_capacity(modules_len),
            pending_threads: Vec::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn module_ids_sorted(&self) -> Vec<ModuleId> {
        let mut ids = self.modules.keys().copied().collect::<Vec<_>>();
        ids.sort_by(|a, b| self.modules[a].name.cmp(&self.modules[b].name));
        ids
    }

    pub fn find_id_by_name(&self, name: &str) -> Option<ModuleId> {
        self.modules
            .iter()
            .find(|(_, m)| m.name == name)
            .map(|(id, _)| *id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<(ModuleId, &ModuleInfo)> {
        self.modules
            .iter()
            .find(|(_, m)| m.name == name)
            .map(|(mid, m)| (*mid, m))
    }

    pub fn module(&self, id: ModuleId) -> Option<&ModuleInfo> {
        self.modules.get(&id)
    }

    pub fn module_mut(&mut self, id: ModuleId) -> Option<&mut ModuleInfo> {
        self.modules.get_mut(&id)
    }

    pub fn by_surface(&self, sid: &SurfaceId) -> Option<&(TargetId, ModuleId)> {
        self.by_surface.get(sid)
    }

    pub fn remove_sid(&mut self, engine: &mut Engine<'_>, sctk: &mut SctkApp, sid: SurfaceId) {
        if let Some((tid, _)) = self.by_surface.remove(&sid) {
            engine.detach_target(&tid);
            sctk.state.remove_surface_by_surface_id(sid);

            self.by_target.remove(&tid);
        }
    }

    pub fn add_id(&mut self, mid: ModuleId, (sid, tid): (SurfaceId, TargetId)) {
        self.by_module.entry(mid).or_default().push(tid);
        self.by_surface.insert(sid, (tid, mid));
        self.by_target.insert(tid, (sid, mid));
    }

    pub fn reap_threads(&mut self) {
        self.pending_threads.retain(|h| !h.is_finished());
    }
}

impl ModuleManager {
    fn discover_and_load_modules(
        cfg: &mut Config,
        config_path: &Path,
        engine: &mut Engine<'_>,
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
        engine: &mut Engine<'_>,
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

    #[allow(clippy::too_many_arguments)]
    fn __realize_module(
        &mut self,
        engine: &mut Engine<'_>,
        sctk: &mut SctkApp,
        tx: &RuntimeSender,
        loop_handle: &mut LoopHandle<SctkState>,
        config: &Config,
        mid: &ModuleId,
        opts: Option<ui::sctk::Options>,
    ) {
        let Some(module) = self.modules.get_mut(mid) else {
            return;
        };

        module.toggled = true;
        let opts_final = match opts {
            Some(o) => o,
            None => {
                let mut o = module.as_ref().manifest().options.clone();
                if let Some(value) = config.get(&module.name) {
                    module.as_mut().apply_config(engine, value, &mut o);
                }
                o
            }
        };

        for (key, factory) in module.as_ref().pipelines() {
            engine.register_pipeline(PipelineKey::Other(key), factory);
        }

        let items = sctk.create_surfaces(opts_final);
        for CreatedSurface { sid, handles, size } in items {
            let tid = engine.attach_target(Arc::new(handles), size);
            self.add_id(*mid, (sid, tid));
        }

        self.add_subscriptions(tx, loop_handle, mid);
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
        tx: &RuntimeSender,
        dispatch_tx: &loop_channel::Sender<(ModuleId, ErasedMsg)>,
        pending_threads: &mut Vec<JoinHandle<()>>,
    ) {
        if let Some(ut) = utask.as_mut() {
            match ut.action() {
                api_utils::Action::ExitOrbit => {
                    tx.send(Event::Dbus(DbusEvent::Exit));
                    return;
                }
                api_utils::Action::ExitModule => {
                    tx.send(Event::Dbus(DbusEvent::Toggle(module.name.clone())));
                    return;
                }
                api_utils::Action::RedrawModule => {
                    tx.send(Event::Ui(event::Ui::ForceRedraw(*mid)));
                }
                api_utils::Action::None => (),
            }

            if let Some(pending) = ut.tasks.take() {
                for task in pending {
                    let mid = *mid;
                    let result_tx = dispatch_tx.clone();
                    let thread = std::thread::Builder::new()
                        .name(format!("orbit-task-{}", mid.0))
                        .spawn(move || {
                            let msg = futures_lite::future::block_on(task);
                            let _ = result_tx.send((mid, msg.clone_for_send()));
                        })
                        .expect("spawn task thread");
                    pending_threads.push(thread);
                }
            }
        }
    }

    pub fn rediscover_modules(
        &mut self,
        engine: &mut Engine<'_>,
        sctk: &mut SctkApp,
        config: &mut Config,
        config_path: &Path,
    ) -> Result<(), String> {
        for module in self.modules.values_mut() {
            if module.is_loaded() {
                module.as_mut().cleanup(engine);
            }
        }

        let all_sids: Vec<_> = self.by_surface.keys().copied().collect();
        self.by_surface.clear();
        sctk.destroy_surfaces(&all_sids);
        for (_mid, tids) in self.by_module.drain() {
            for tid in tids {
                engine.detach_target(&tid);
            }
        }

        for (_mid, handles) in self.dispatch_tokens.drain() {
            for handle in handles {
                self.pending_threads.push(handle.thread);
            }
        }
        self.reap_threads();

        self.modules = Self::discover_and_load_modules(config, config_path, engine, None)?;
        Ok(())
    }

    pub fn add_subscriptions(
        &mut self,
        tx: &RuntimeSender,
        loop_handle: &mut LoopHandle<SctkState>,
        mid: &ModuleId,
    ) {
        let Some(module) = self.modules.get_mut(mid) else {
            return;
        };
        if !module.toggled {
            return;
        }

        let usub = api_utils::unravel_sub(module.as_ref().subscriptions());

        let mut tokens = Vec::new();
        super::subscriptions::handle_subs(usub.subs, tx, loop_handle, mid, &mut tokens);
        if !tokens.is_empty() {
            self.sub_tokens.entry(*mid).or_default().append(&mut tokens);
        }

        let mut handles = Vec::new();
        super::subscriptions::handle_streams(usub.streams, tx, loop_handle, mid, &mut handles);
        if !tokens.is_empty() {
            self.dispatch_tokens
                .entry(*mid)
                .or_default()
                .append(&mut handles);
        }
    }

    pub fn remove_subscriptions(
        &mut self,
        loop_handle: &mut LoopHandle<SctkState>,
        mid: &ModuleId,
    ) {
        if let Some(tokens) = self.sub_tokens.remove(mid) {
            for token in tokens {
                loop_handle.remove(token);
            }
        }
    }

    pub fn remove_streams(&mut self, loop_handle: &mut LoopHandle<SctkState>, mid: &ModuleId) {
        if let Some(tokens) = self.dispatch_tokens.remove(mid) {
            for handle in tokens {
                loop_handle.remove(handle.rx_token);
                self.pending_threads.push(handle.thread);
            }
        }
    }

    pub fn load_module(
        engine: &mut Engine<'_>,
        map: Option<&serde_yml::Value>,
        module: &mut ModuleInfo,
    ) -> Result<(), String> {
        module.ensure_loaded()?;
        module
            .as_ref()
            .validate_config(map.unwrap_or(&serde_yml::Value::Null))?;

        for (key, factory) in module.as_ref().pipelines() {
            engine.register_pipeline(PipelineKey::Other(key), factory);
        }

        Ok(())
    }

    pub fn realize_toggled_modules(
        &mut self,
        engine: &mut Engine<'_>,
        sctk: &mut SctkApp,
        tx: &RuntimeSender,
        loop_handle: &mut LoopHandle<SctkState>,
        config: &Config,
    ) {
        for mid in self.modules.keys().copied().collect::<Vec<ModuleId>>() {
            if self.modules[&mid].toggled {
                self.realize_module(engine, sctk, tx, loop_handle, config, &mid);
            }
        }
    }

    pub fn realize_module(
        &mut self,
        engine: &mut Engine<'_>,
        sctk: &mut SctkApp,
        tx: &RuntimeSender,
        loop_handle: &mut LoopHandle<SctkState>,
        config: &Config,
        mid: &ModuleId,
    ) {
        self.__realize_module(engine, sctk, tx, loop_handle, config, mid, None);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn realize_module_with_opts(
        &mut self,
        engine: &mut Engine<'_>,
        sctk: &mut SctkApp,
        tx: &RuntimeSender,
        loop_handle: &mut LoopHandle<SctkState>,
        config: &Config,
        mid: &ModuleId,
        opts: ui::sctk::Options,
    ) {
        self.__realize_module(engine, sctk, tx, loop_handle, config, mid, Some(opts));
    }

    pub fn unrealize_module(
        &mut self,
        engine: &mut Engine<'_>,
        sctk: &mut SctkApp,
        loop_handle: &mut LoopHandle<SctkState>,
        mid: &ModuleId,
    ) {
        if let Some(module) = self.modules.get_mut(mid) {
            module.toggled = false;
        };

        let sids: Vec<SurfaceId> = self
            .by_surface
            .iter()
            .filter_map(|(sid, (_, owner))| if owner == mid { Some(*sid) } else { None })
            .collect();
        let tids = self.by_module.remove(mid).unwrap_or_default();

        for tid in tids {
            engine.detach_target(&tid);
        }
        self.by_surface.retain(|_, (_, owner)| owner != mid);
        self.by_target.retain(|_, (_, owner)| owner != mid);

        self.remove_subscriptions(loop_handle, mid);
        self.remove_streams(loop_handle, mid);
        self.reap_threads();

        sctk.destroy_surfaces(&sids);
    }

    pub fn handle_platform_event(
        &mut self,
        engine: &mut Engine,
        tx: &RuntimeSender,
        task_tx: &loop_channel::Sender<(ModuleId, ErasedMsg)>,
        event: &SctkEvent,
        id: Option<(ModuleId, Option<TargetId>)>,
    ) {
        #![allow(clippy::too_many_arguments)]
        fn handle_platform_event_internal(
            engine: &mut Engine,
            tx: &RuntimeSender,
            task_tx: &loop_channel::Sender<(ModuleId, ErasedMsg)>,
            event: &SctkEvent,
            mid: &ModuleId,
            module: &mut ModuleInfo,
            tid: &TargetId,
            pending_threads: &mut Vec<JoinHandle<()>>,
        ) {
            let mut task = None;
            engine.handle_platform_event(
                tid,
                event,
                &mut ModuleManager::do_update,
                &mut (module, &mut task),
                tid,
            );
            ModuleManager::handle_task(task, mid, module, tx, task_tx, pending_threads);
        }

        if let Some((mid, o_tid)) = id
            && let Some(module) = self.modules.get_mut(&mid)
        {
            if let Some(tid) = o_tid {
                handle_platform_event_internal(
                    engine,
                    tx,
                    task_tx,
                    event,
                    &mid,
                    module,
                    &tid,
                    &mut self.pending_threads,
                );
            } else {
                let Some(targets) = self.by_module.get(&mid) else {
                    // TODO: this shouldn't happen so maybe emit a warning or something?
                    return;
                };
                for tid in targets.clone() {
                    handle_platform_event_internal(
                        engine,
                        tx,
                        task_tx,
                        event,
                        &mid,
                        module,
                        &tid,
                        &mut self.pending_threads,
                    );
                }
            }
        } else {
            for (mid, module) in self.modules.iter_mut() {
                let Some(targets) = self.by_module.get(mid) else {
                    // TODO: this shouldn't happen so maybe emit a warning or something?
                    continue;
                };
                for tid in targets.clone() {
                    handle_platform_event_internal(
                        engine,
                        tx,
                        task_tx,
                        event,
                        mid,
                        module,
                        &tid,
                        &mut self.pending_threads,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_target(
        &mut self,
        engine: &mut Engine,
        sctk: &mut SctkApp,
        tx: &RuntimeSender,
        task_tx: &loop_channel::Sender<(ModuleId, ErasedMsg)>,
        mid: &ModuleId,
        tid: &TargetId,
        poll_override: bool,
    ) {
        if let Some(module) = self.modules.get_mut(mid) {
            let Some((sid, _)) = self.by_target.get(tid) else {
                return;
            };
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
            Self::handle_task(task, mid, module, tx, task_tx, &mut self.pending_threads);
        }
    }

    pub fn render_module(
        &mut self,
        engine: &mut Engine,
        sctk: &mut SctkApp,
        tx: &RuntimeSender,
        task_tx: &loop_channel::Sender<(ModuleId, ErasedMsg)>,
        mid: &ModuleId,
        poll_override: bool,
    ) {
        let Some(targets) = self.by_module.get(mid) else {
            return;
        };
        let targets = targets.to_vec();
        for tid in targets {
            self.render_target(engine, sctk, tx, task_tx, mid, &tid, poll_override);
        }
    }

    pub fn render(
        &mut self,
        engine: &mut Engine,
        sctk: &mut SctkApp,
        tx: &RuntimeSender,
        task_tx: &loop_channel::Sender<(ModuleId, ErasedMsg)>,
        poll_override: bool,
    ) {
        let modules = self.modules.keys().copied().collect::<Vec<ModuleId>>();
        for mid in modules {
            self.render_module(engine, sctk, tx, task_tx, &mid, poll_override);
        }
    }
}
