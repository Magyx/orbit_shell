use std::sync::{Arc, mpsc};

use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    reexports::calloop::channel as loop_channel,
    registry::RegistryState,
    seat::SeatState,
    session_lock::SessionLockState,
    shell::{WaylandSurface, wlr_layer::LayerShell, xdg::XdgShell},
};
use ui::{
    model::Size,
    sctk::{Options, RawWaylandHandles, SctkEvent, SurfaceId, erased, handler, state::SctkState},
};
use wayland_client::{Connection, EventQueue, Proxy, QueueHandle, globals::registry_queue_init};

use crate::event::SctkMessage;

pub fn take_erased_from_message(evt: &ui::sctk::SctkEvent) -> Option<orbit_api::ErasedMsg> {
    if let ui::sctk::SctkEvent::Message(arc) = evt {
        let mut guard = arc.lock().unwrap();
        guard
            .take()
            .and_then(|boxed| boxed.downcast::<orbit_api::ErasedMsg>().ok())
            .map(|bx| *bx)
    } else {
        None
    }
}

pub struct OrbitHandler;

impl handler::SctkHandler<SctkMessage> for OrbitHandler {
    fn new_output(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        _output: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput,
    ) -> handler::Emit<SctkMessage> {
        handler::Emit::One(SctkMessage::OutputCreated)
    }
    fn update_output(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        _output: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput,
    ) -> handler::Emit<SctkMessage> {
        handler::Emit::One(SctkMessage::OutputCreated)
    }
    fn closed(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        layer: &smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    ) -> handler::Emit<SctkMessage> {
        handler::Emit::One(SctkMessage::SurfaceDestroyed(
            layer.wl_surface().id().protocol_id(),
        ))
    }

    fn request_close(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        window: &smithay_client_toolkit::shell::xdg::window::Window,
    ) -> handler::Emit<SctkMessage> {
        handler::Emit::One(SctkMessage::SurfaceDestroyed(
            window.wl_surface().id().protocol_id(),
        ))
    }
}

#[derive(Clone, Debug)]
pub struct CreatedSurface {
    pub sid: SurfaceId,
    pub handles: RawWaylandHandles,
    pub size: Size<u32>,
}

pub struct SctkApp {
    pub conn: Connection,
    pub event_queue: Option<EventQueue<SctkState>>,
    pub qh: QueueHandle<SctkState>,
    pub state: SctkState,
}

impl SctkApp {
    pub fn new(
        main_tx: mpsc::Sender<crate::Event>,
    ) -> Result<(loop_channel::Channel<SctkEvent>, Self), &'static str> {
        let conn = Connection::connect_to_env().map_err(|_| "err")?;
        let (globals, event_queue) = registry_queue_init(&conn).map_err(|_| "err")?;
        let qh: QueueHandle<SctkState> = event_queue.handle();

        let sctk_handler = erased::erase::<OrbitHandler, SctkMessage, _>(move |e| {
            let _ = main_tx.send(crate::Event::Ui(crate::event::Ui::Orbit(e)));
        });

        let registry = RegistryState::new(&globals);
        let compositor = CompositorState::bind(&globals, &qh).map_err(|_| "err")?;
        let outputs = OutputState::new(&globals, &qh);
        let seats = SeatState::new(&globals, &qh);
        let layer_shell = LayerShell::bind(&globals, &qh).map_err(|_| "err")?;
        let xdg_shell = XdgShell::bind(&globals, &qh).map_err(|_| "err")?;
        let session_lock = SessionLockState::new(&globals, &qh);
        let (tx, rx) = loop_channel::channel::<SctkEvent>();

        let state = SctkState::new(
            compositor,
            Some(layer_shell),
            Some(xdg_shell),
            outputs,
            seats,
            registry,
            session_lock,
            sctk_handler,
            tx,
        );

        Ok((
            rx,
            Self {
                conn,
                event_queue: Some(event_queue),
                qh,
                state,
            },
        ))
    }

    pub fn take_event_queue(&mut self) -> EventQueue<SctkState> {
        self.event_queue.take().expect("event_queue already taken")
    }

    pub fn create_surfaces(&mut self, opts: Options) -> Vec<CreatedSurface> {
        let mut items = Vec::new();
        match opts {
            Options::Layer(layer_opts) => {
                for (sid, size) in self.state.spawn_layer_surfaces(&self.qh, layer_opts) {
                    let handles =
                        RawWaylandHandles::new(&self.conn, &self.state.surfaces[&sid].wl_surface);
                    items.push(CreatedSurface { sid, handles, size });
                }
            }
            Options::Xdg(xdg_opts) => {
                let (sid, size) = self.state.spawn_window(&self.qh, xdg_opts);
                let handles =
                    RawWaylandHandles::new(&self.conn, &self.state.surfaces[&sid].wl_surface);
                items.push(CreatedSurface { sid, handles, size });
            }
            ui::sctk::Options::Lock(lock_opts) => _ = self.state.lock_session(&self.qh, lock_opts),
        }
        items
    }

    pub fn ensure_surfaces(
        &mut self,
        opts: &Options,
    ) -> Vec<(SurfaceId, Arc<RawWaylandHandles>, Size<u32>)> {
        let new_surfaces = match opts {
            Options::Layer(layer_options) => {
                self.state.ensure_layer_surfaces(&self.qh, layer_options)
            }
            Options::Lock(lock_options) => self
                .state
                .ensure_lock_surfaces(&self.qh, lock_options)
                .unwrap_or_default(),
            _ => {
                return vec![];
            }
        };

        let mut out: Vec<(SurfaceId, Arc<RawWaylandHandles>, Size<u32>)> = Vec::new();
        for (sid, size) in new_surfaces {
            let target = Arc::new(RawWaylandHandles::new(
                &self.conn,
                &self.state.surfaces[&sid].wl_surface,
            ));
            out.push((sid, target, size));
        }
        out
    }

    pub fn destroy_surfaces(&mut self, sids: &[SurfaceId]) {
        for sid in sids {
            self.state.remove_surface_by_surface_id(*sid);
        }
    }
}
