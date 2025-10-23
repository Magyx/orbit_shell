use orbit_api::ErasedMsg;
use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    reexports::calloop::channel as loop_channel,
    registry::RegistryState,
    seat::SeatState,
    session_lock::SessionLockState,
    shell::{wlr_layer::LayerShell, xdg::XdgShell},
};
use ui::{
    model::Size,
    sctk::{
        DefaultHandler, Options, RawWaylandHandles, SctkEvent, SurfaceId, adapter, state::SctkState,
    },
};
use wayland_client::{Connection, EventQueue, QueueHandle, globals::registry_queue_init};

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
    pub fn new() -> Result<(loop_channel::Channel<SctkEvent>, Self), &'static str> {
        let conn = Connection::connect_to_env().map_err(|_| "err")?;
        let (globals, event_queue) = registry_queue_init(&conn).map_err(|_| "err")?;
        let qh: QueueHandle<SctkState> = event_queue.handle();

        let (tx, rx) = loop_channel::channel::<SctkEvent>();

        let sctk_handler_tx = tx.clone();
        let sctk_handler = adapter::erase::<DefaultHandler, ErasedMsg, _>(move |m| {
            let _ = sctk_handler_tx.send(SctkEvent::message(m));
        });

        let registry = RegistryState::new(&globals);
        let compositor = CompositorState::bind(&globals, &qh).map_err(|_| "err")?;
        let outputs = OutputState::new(&globals, &qh);
        let seats = SeatState::new(&globals, &qh);
        let layer_shell = LayerShell::bind(&globals, &qh).map_err(|_| "err")?;
        let xdg_shell = XdgShell::bind(&globals, &qh).map_err(|_| "err")?;
        let session_lock = SessionLockState::new(&globals, &qh);

        let state = SctkState::new(
            compositor,
            Some(layer_shell),
            Some(xdg_shell),
            outputs,
            seats,
            registry,
            session_lock,
            sctk_handler,
            tx.clone(),
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

    pub fn take_event_queu(&mut self) -> EventQueue<SctkState> {
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
        }
        items
    }

    pub fn destroy_surfaces(&mut self, sids: &[SurfaceId]) {
        for sid in sids {
            self.state.remove_surface_by_surface_id(*sid);
        }

        self.state.needs_redraw = true;
    }
}
