use orbit_dbus::DbusEvent;
use ui::sctk::SctkEvent;

use crate::config::ConfigEvent;

#[derive(Debug)]
pub enum Event {
    Sctk(SctkEvent),
    Dbus(DbusEvent),
    Config(ConfigEvent),
}
