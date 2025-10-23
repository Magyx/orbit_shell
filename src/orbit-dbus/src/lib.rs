pub const DESTINATION: &str = "io.github.orbitshell.Orbit1";
pub const OBJECT_PATH: &str = "/io/github/orbitshell/Orbit1";
pub const INTERFACE: &str = "io.github.orbitshell.Orbit1";

#[derive(Debug, Clone)]
pub enum DbusEvent {
    Reload(std::sync::mpsc::Sender<String>),
    Modules(std::sync::mpsc::Sender<String>),
    Toggle(String),
    Exit,
}
