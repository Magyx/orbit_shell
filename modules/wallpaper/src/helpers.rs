use std::{env, path::PathBuf};

pub fn xdg_home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}
