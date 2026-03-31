// TODO: better error messages cmon dude
use std::{fmt::Write, path::PathBuf, sync::mpsc};

use calloop::{EventLoop, channel as loop_channel, futures::executor};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use ui::sctk::{SctkEvent, SurfaceId, state::SctkState};

use orbit_api::{Engine, ErasedMsg};
use orbit_dbus::DbusEvent;

use crate::{
    config::{Config, ConfigEvent, ConfigInstruction, ConfigWatcher, compare_configs},
    dialog::ErrorDialog,
    module::ModuleInfo,
    module_manager::ModuleManager,
    sctk::SctkApp,
};

use {dbus::OrbitdServer, event::Event, module::ModuleId};

mod api_utils;
mod config;
mod dbus;
mod dialog;
mod event;
mod module;
mod module_manager;
mod sctk;
mod subscriptions;
mod trace;

struct Orbit<'a> {
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,

    dbus_rx: Option<loop_channel::Channel<DbusEvent>>,
    d_server: OrbitdServer,

    config_path: PathBuf,
    config: Config,
    config_rx: Option<loop_channel::Channel<ConfigEvent>>,
    config_watcher: ConfigWatcher,

    sctk_rx: Option<loop_channel::Channel<SctkEvent>>,
    sctk: SctkApp,

    engine: Engine<'a>,
    error_dialog: ErrorDialog,
    module_manager: ModuleManager,
}

impl<'a> Orbit<'a> {
    fn new(config_path: Option<PathBuf>) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<Event>();

        let (dbus_rx, d_server) = OrbitdServer::new();

        let config_path = config_path.unwrap_or_else(config::xdg_config_home);
        let mut config = config::load_cfg(&config_path)?;
        let (config_rx, config_watcher) = ConfigWatcher::new();

        let (sctk_rx, sctk) = SctkApp::new(tx.clone())?;

        let mut engine = Engine::default();
        let module_manager = ModuleManager::new(&mut config, &config_path, &mut engine)?;

        Ok(Self {
            tx,
            rx,

            dbus_rx: Some(dbus_rx),
            d_server,

            config_path,
            config,
            config_rx: Some(config_rx),
            config_watcher,

            sctk_rx: Some(sctk_rx),
            sctk,

            engine,
            error_dialog: ErrorDialog::new(),
            module_manager,
        })
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

            for mid in self.module_manager.module_ids_sorted() {
                push_commands(
                    &mut reply,
                    self.module_manager.module(mid).expect("just found"),
                    1,
                );
            }
        } else if let Some(mid) = self.module_manager.find_id_by_name(module_name) {
            push_commands(
                &mut reply,
                self.module_manager.module(mid).expect("just found"),
                0,
            );
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

        self.module_manager.realize_toggled_modules(
            &mut self.engine,
            &mut self.sctk,
            &tx,
            &mut event_loop.handle(),
            &self.config,
        );

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
                                    self.module_manager.remove_sid(
                                        &mut self.engine,
                                        &mut self.sctk,
                                        sid,
                                    );
                                }
                                event::SctkMessage::OutputCreated => {
                                    for mid in self.module_manager.module_ids_sorted() {
                                        let module =
                                            self.module_manager.module(mid).expect("just found");

                                        if !module.is_loaded() || !module.toggled {
                                            continue;
                                        }

                                        let new_surfaces = self
                                            .sctk
                                            .ensure_surfaces(&module.as_ref().manifest().options);

                                        for (sid, target, size) in new_surfaces {
                                            let tid = self.engine.attach_target(target, size);
                                            self.module_manager.add_id(mid, (sid, tid));
                                        }
                                    }
                                }
                            },
                            event::Ui::Sctk(sctk_event) => {
                                if let Some(sid) = sctk_event.surface_id() {
                                    if let Some(&(tid, mid)) = self.module_manager.by_surface(&sid)
                                    {
                                        self.module_manager.handle_platform_event(
                                            &mut self.engine,
                                            &tx,
                                            &task_scheduler,
                                            &sctk_event,
                                            Some((mid, Some(tid))),
                                        );
                                    } else {
                                        self.error_dialog
                                            .handle_platform_event(&mut self.engine, &sctk_event);
                                    }
                                } else {
                                    self.module_manager.handle_platform_event(
                                        &mut self.engine,
                                        &tx,
                                        &task_scheduler,
                                        &sctk_event,
                                        None,
                                    );
                                }
                            }
                            event::Ui::Module(mid, sctk_event) => {
                                let event = if let Some(base) =
                                    sctk::take_erased_from_message(&sctk_event)
                                {
                                    ui::sctk::SctkEvent::message(base)
                                } else {
                                    sctk_event
                                };

                                self.module_manager.handle_platform_event(
                                    &mut self.engine,
                                    &tx,
                                    &task_scheduler,
                                    &event,
                                    Some((mid, None)),
                                );
                            }
                            event::Ui::ForceRedraw(mid) => {
                                self.module_manager.render_module(
                                    &mut self.engine,
                                    &mut self.sctk,
                                    &tx,
                                    &task_scheduler,
                                    &mid,
                                    true,
                                );
                            }
                        }
                    }
                    Event::Dbus(dbus_event) => match dbus_event {
                        DbusEvent::Reload(resp_tx) => {
                            let resp = match self.module_manager.rediscover_modules(
                                &mut self.engine,
                                &mut self.sctk,
                                &mut self.config,
                                &self.config_path,
                            ) {
                                Ok(_) => {
                                    self.module_manager.realize_toggled_modules(
                                        &mut self.engine,
                                        &mut self.sctk,
                                        &tx,
                                        &mut event_loop.handle(),
                                        &self.config,
                                    );
                                    "Reloaded".into()
                                }
                                Err(e) => e,
                            };

                            let _ = resp_tx.send(resp);
                        }
                        DbusEvent::Modules(resp_tx) => {
                            let mut reply =
                                String::with_capacity(128 + self.module_manager.len() * 32);
                            reply.push_str("Loaded modules:\n");

                            for mid in self.module_manager.module_ids_sorted() {
                                let module = self.module_manager.module(mid).expect("just found");
                                let loaded = if module.is_loaded() {
                                    "loaded"
                                } else {
                                    "unloaded"
                                };
                                let shown = if module.toggled { ", shown" } else { "" };
                                writeln!(reply, "\t{} ({}{})", module.name, loaded, shown).unwrap();
                            }

                            _ = resp_tx.send(reply);
                        }
                        DbusEvent::Commands(module_name, resp_tx) => {
                            _ = resp_tx.send(self.format_commands(&module_name));
                        }
                        DbusEvent::Toggle(module_name) => {
                            let Some((mid, module)) =
                                self.module_manager.find_by_name(&module_name)
                            else {
                                continue;
                            };

                            if module.toggled {
                                self.module_manager.unrealize_module(
                                    &mut self.engine,
                                    &mut self.sctk,
                                    &mut event_loop.handle(),
                                    &mid,
                                );
                            } else {
                                self.module_manager.realize_module(
                                    &mut self.engine,
                                    &mut self.sctk,
                                    &tx,
                                    &mut event_loop.handle(),
                                    &self.config,
                                    &mid,
                                );
                            }
                        }
                        DbusEvent::Command(module_name, command_name) => {
                            let Some((mid, module)) =
                                self.module_manager.find_by_name(&module_name)
                            else {
                                tracing::warn!(module = %module_name, command = %command_name, "command for unknown module");
                                continue;
                            };

                            if !module.is_loaded() {
                                tracing::warn!(module = %module_name, "module is not loaded");
                                continue;
                            }

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
                            if self.config == new_config && !self.error_dialog.is_shown() {
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
                            for (
                                name,
                                ConfigInstruction {
                                    should_unrealize,
                                    should_realize,
                                    config_changed,
                                },
                            ) in instructions
                            {
                                let Some(mid) = self.module_manager.find_id_by_name(&name) else {
                                    errors.push(format!("Module {} could not be found.", &name));
                                    continue;
                                };

                                if should_unrealize {
                                    self.module_manager.unrealize_module(
                                        &mut self.engine,
                                        &mut self.sctk,
                                        &mut event_loop.handle(),
                                        &mid,
                                    );
                                    if let Some(module) = self.module_manager.module_mut(mid) {
                                        module.unload(&mut self.engine);
                                    }
                                }

                                if (should_realize || config_changed)
                                    && let Some(module) = self.module_manager.module_mut(mid)
                                    && let Err(e) = ModuleManager::load_module(
                                        &mut self.engine,
                                        new_config.get(&name),
                                        module,
                                    )
                                {
                                    errors.push(e);
                                    continue;
                                }

                                if should_realize {
                                    let show_on_startup = self
                                        .module_manager
                                        .module(mid)
                                        .map(|m| m.as_ref().manifest().show_on_startup)
                                        .unwrap_or(false);

                                    if show_on_startup {
                                        self.module_manager.realize_module(
                                            &mut self.engine,
                                            &mut self.sctk,
                                            &tx,
                                            &mut event_loop.handle(),
                                            &new_config,
                                            &mid,
                                        );
                                    }
                                }

                                if !should_realize
                                    && config_changed
                                    && new_config.enabled(&name)
                                    && let Some(config) = new_config.get(&name)
                                    && let Some(module) = self.module_manager.module_mut(mid)
                                {
                                    let mut opts = module.as_ref().manifest().options.clone();
                                    let must_rebuild = module.as_mut().apply_config(
                                        &mut self.engine,
                                        config,
                                        &mut opts,
                                    );

                                    if must_rebuild {
                                        self.module_manager.unrealize_module(
                                            &mut self.engine,
                                            &mut self.sctk,
                                            &mut event_loop.handle(),
                                            &mid,
                                        );
                                        self.module_manager.realize_module_with_opts(
                                            &mut self.engine,
                                            &mut self.sctk,
                                            &tx,
                                            &mut event_loop.handle(),
                                            &new_config,
                                            &mid,
                                            opts,
                                        );
                                    } else {
                                        self.module_manager
                                            .remove_subscriptions(&mut event_loop.handle(), &mid);
                                        self.module_manager.add_subscriptions(
                                            &tx,
                                            &mut event_loop.handle(),
                                            &mid,
                                        );
                                        let _ = tx.send(Event::Ui(event::Ui::ForceRedraw(mid)));
                                    }
                                }
                            }

                            if errors.is_empty() {
                                self.config = new_config;
                                self.error_dialog.hide(&mut self.engine, &mut self.sctk);
                            } else if !errors.is_empty() {
                                let _ = tx.send(Event::Config(ConfigEvent::Err(errors)));
                            }
                        }
                        ConfigEvent::Err(errors) => {
                            tracing::warn!(?errors, "config errors");
                            self.error_dialog
                                .show(&mut self.engine, &mut self.sctk, errors);
                        }
                    },
                }
            }

            if need_tick {
                self.module_manager.render(
                    &mut self.engine,
                    &mut self.sctk,
                    &tx,
                    &task_scheduler,
                    false,
                );
                self.error_dialog.render(&mut self.engine);
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
