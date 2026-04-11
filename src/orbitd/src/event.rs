use std::sync::mpsc;

use orbit_api::ErasedMsg;
use orbit_dbus::DbusEvent;
use ui::sctk::SctkEvent;

use crate::{config::ConfigEvent, module::ModuleId};

pub struct RuntimeSender {
    tx: mpsc::Sender<Event>,
    loop_signal: calloop::LoopSignal,
}

impl RuntimeSender {
    pub(crate) fn new(tx: mpsc::Sender<Event>, loop_signal: calloop::LoopSignal) -> Self {
        Self { tx, loop_signal }
    }

    pub fn send(&self, event: Event) {
        if self.tx.send(event).is_ok() {
            self.loop_signal.wakeup();
        }
    }
}

impl Clone for RuntimeSender {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            loop_signal: self.loop_signal.clone(),
        }
    }
}

#[derive(Debug)]
pub enum Event {
    Ui(Ui),
    Dbus(DbusEvent),
    Config(ConfigEvent),
}

#[derive(Debug)]
pub enum SctkMessage {
    OutputCreated,
    SurfaceDestroyed(u32),
    SurfaceConfigured(u32),
}

#[derive(Debug)]
pub enum FromDispatch {
    Subscription,
    Task,
}

#[derive(Debug)]
pub enum Ui {
    Orbit(SctkMessage),
    Sctk(SctkEvent),
    Module(ModuleId, SctkEvent),
    Result(FromDispatch, ModuleId, ErasedMsg),
    ForceRedraw(ModuleId),
}
