use std::{
    sync::mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};

use calloop::channel as loop_channel;
use orbit_dbus::{DESTINATION, DbusEvent, OBJECT_PATH};
use zbus::{blocking::connection::Builder, interface};

pub struct OrbitdServer {
    tx: loop_channel::Sender<DbusEvent>,
    handle: Option<JoinHandle<()>>,
    stop_tx: Option<mpsc::Sender<()>>,
}

impl OrbitdServer {
    pub fn new() -> (loop_channel::Channel<DbusEvent>, Self) {
        let (tx, rx) = loop_channel::channel();
        (
            rx,
            Self {
                tx,
                handle: None,
                stop_tx: None,
            },
        )
    }

    pub fn start(&mut self) {
        assert!(self.handle.is_none(), "orbitd dbus server already started");

        let (stop_tx, stop_rx) = mpsc::channel();
        self.stop_tx = Some(stop_tx);

        let tx = self.tx.clone();

        let handle = thread::spawn(move || {
            let run = async move {
                let iface = OrbitIface::new(tx);

                let conn = Builder::session()?
                    .name(DESTINATION)?
                    .serve_at(OBJECT_PATH, iface)?
                    .build()?;

                let _ = stop_rx.recv();
                conn.graceful_shutdown();

                Ok::<(), zbus::Error>(())
            };

            if let Err(e) = futures_lite::future::block_on(run) {
                eprintln!("zbus server failed: {e}");
            }
        });

        self.handle = Some(handle);
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for OrbitdServer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct OrbitIface {
    tx: loop_channel::Sender<DbusEvent>,
}

impl OrbitIface {
    fn new(tx: loop_channel::Sender<DbusEvent>) -> Self {
        Self { tx }
    }
}

#[interface(name = "io.github.orbitshell.Orbit1")]
impl OrbitIface {
    fn alive(&self) {}
    fn reload(&self) -> String {
        let (resp_tx, resp_rx) = mpsc::channel::<String>();
        let _ = self.tx.send(DbusEvent::Reload(resp_tx));

        resp_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or("timeout or no response".into())
    }
    fn modules(&self) -> String {
        let (resp_tx, resp_rx) = mpsc::channel::<String>();
        let _ = self.tx.send(DbusEvent::Modules(resp_tx));

        resp_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or("timeout or no response".into())
    }
    fn toggle(&self, module: &str) {
        let _ = self.tx.send(DbusEvent::Toggle(module.to_string()));
    }
    fn exit(&self) {
        let _ = self.tx.send(DbusEvent::Exit);
    }
}
