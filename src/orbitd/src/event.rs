use orbit_api::ErasedMsg;
use orbit_dbus::DbusEvent;
use ui::sctk::SctkEvent;

use crate::{config::ConfigEvent, module::ModuleId};

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
