use std::sync::mpsc;

use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    reexports::calloop::channel as loop_channel,
    registry::RegistryState,
    seat::SeatState,
    session_lock::SessionLockState,
    shell::{WaylandSurface, wlr_layer::LayerShell, xdg::XdgShell},
};
use ui::sctk::{Options, SctkEvent, SurfaceId, erased, handler, state::SctkState};
use wayland_client::{
    Connection, EventQueue, Proxy, QueueHandle, globals::registry_queue_init,
    protocol::wl_output::WlOutput,
};

use crate::event::OrbitMessage;

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

impl handler::SctkHandler<OrbitMessage> for OrbitHandler {
    fn layer_configure(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        layer: &smithay_client_toolkit::shell::wlr_layer::LayerSurface,
        _configure: smithay_client_toolkit::shell::wlr_layer::LayerSurfaceConfigure,
        _serial: u32,
    ) -> handler::Emit<OrbitMessage> {
        handler::Emit::One(OrbitMessage::SurfaceConfigured(
            layer.wl_surface().id().protocol_id(),
        ))
    }
    fn window_configure(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        window: &smithay_client_toolkit::shell::xdg::window::Window,
        _configure: smithay_client_toolkit::shell::xdg::window::WindowConfigure,
        _serial: u32,
    ) -> handler::Emit<OrbitMessage> {
        handler::Emit::One(OrbitMessage::SurfaceConfigured(
            window.wl_surface().id().protocol_id(),
        ))
    }
    fn lock_configure(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        surface: smithay_client_toolkit::session_lock::SessionLockSurface,
        _configure: smithay_client_toolkit::session_lock::SessionLockSurfaceConfigure,
        _serial: u32,
    ) -> handler::Emit<OrbitMessage> {
        handler::Emit::One(OrbitMessage::SurfaceConfigured(
            surface.wl_surface().id().protocol_id(),
        ))
    }

    fn new_output(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        _output: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput,
    ) -> handler::Emit<OrbitMessage> {
        handler::Emit::One(OrbitMessage::OutputCreated)
    }
    fn update_output(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        _output: smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput,
    ) -> handler::Emit<OrbitMessage> {
        handler::Emit::One(OrbitMessage::OutputCreated)
    }

    fn closed(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        layer: &smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    ) -> handler::Emit<OrbitMessage> {
        handler::Emit::One(OrbitMessage::SurfaceDestroyed(
            layer.wl_surface().id().protocol_id(),
        ))
    }
    fn request_close(
        _conn: &Connection,
        _qh: &QueueHandle<ui::sctk::state::SctkState>,
        window: &smithay_client_toolkit::shell::xdg::window::Window,
    ) -> handler::Emit<OrbitMessage> {
        handler::Emit::One(OrbitMessage::SurfaceDestroyed(
            window.wl_surface().id().protocol_id(),
        ))
    }
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
    ) -> Result<(loop_channel::Channel<SctkEvent>, Self), ui::Error> {
        let conn = Connection::connect_to_env().map_err(ui::error::SctkError::connect)?;
        let (globals, event_queue) =
            registry_queue_init(&conn).map_err(ui::error::SctkError::registry_init)?;
        let qh: QueueHandle<SctkState> = event_queue.handle();

        let sctk_handler = erased::erase::<OrbitHandler, OrbitMessage, _>(move |e| {
            let _ = main_tx.send(crate::Event::Ui(crate::event::Ui::Orbit(e)));
        });

        let registry = RegistryState::new(&globals);
        let compositor =
            CompositorState::bind(&globals, &qh).map_err(ui::error::SctkError::bind_global)?;
        let outputs = OutputState::new(&globals, &qh);
        let seats = SeatState::new(&globals, &qh);
        let layer_shell =
            LayerShell::bind(&globals, &qh).map_err(ui::error::SctkError::bind_global)?;
        let xdg_shell = XdgShell::bind(&globals, &qh).map_err(ui::error::SctkError::bind_global)?;
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

    pub fn create_surfaces(&mut self, opts: Options) -> Vec<SurfaceId> {
        match opts {
            Options::Layer(layer_opts) => self.state.spawn_layer_surfaces(&self.qh, layer_opts),
            Options::Xdg(xdg_opts) => {
                vec![self.state.spawn_window(&self.qh, xdg_opts)]
            }
            Options::Lock(lock_opts) => self
                .state
                .lock_session(&self.qh, lock_opts)
                .unwrap_or_else(|e| {
                    tracing::error!("failed to create lock session surfaces: {e:?}");
                    vec![]
                }),
        }
    }

    pub fn ensure_surfaces(&mut self, opts: &Options) -> Vec<SurfaceId> {
        match opts {
            Options::Layer(layer_options) => self
                .state
                .ensure_layer_surfaces(&self.qh, layer_options)
                .iter()
                .map(|(s, _)| s)
                .copied()
                .collect(),
            Options::Lock(lock_options) => self
                .state
                .ensure_lock_surfaces(&self.qh, lock_options)
                .unwrap_or_else(|e| {
                    tracing::error!("failed to ensure lock session surfaces: {e:?}");
                    vec![]
                })
                .iter()
                .map(|(s, _)| s)
                .copied()
                .collect(),
            _ => {
                vec![]
            }
        }
    }

    pub fn destroy_surfaces(&mut self, sids: &[SurfaceId]) {
        for sid in sids {
            self.state.remove_surface_by_surface_id(*sid);
        }
    }

    pub fn get_output(&self, sid: &SurfaceId) -> Option<&WlOutput> {
        self.state.surfaces.get(sid).and_then(|r| r.output.as_ref())
    }
}
