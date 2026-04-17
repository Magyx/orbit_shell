use std::path::{Path, PathBuf};

/// The system-wide orbit modules directory, installed by the package manager.
/// Files here are assumed to be root-owned and safe to load without further
/// permission checks.
pub const SYSTEM_MODULES_DIR: &str = "/usr/lib/orbit/modules";

/// Returns `~/.config/orbit` (or `$XDG_CONFIG_HOME/orbit`).
pub fn config_home() -> PathBuf {
    let base = xdg::BaseDirectories::new().config_home.unwrap_or_default();
    base.join("orbit")
}

/// Returns the default user modules directory: `<config_home>/modules`.
/// This is the path that is checked when no override is present in the config.
pub fn default_user_modules_dir(config_home: &Path) -> PathBuf {
    config_home.join("modules")
}
