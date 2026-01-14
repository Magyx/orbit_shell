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
pub enum Ui {
    Sctk(SctkEvent),
    Orbit(ModuleId, SctkEvent),
    ForceRedraw(ModuleId),
}
