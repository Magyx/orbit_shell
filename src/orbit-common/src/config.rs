use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde_yml::{Mapping, Value};

#[derive(Default, Debug)]
pub struct Config {
    /// `modules: { bar: true, launcher: false }`. The enable/disable map.
    pub modules: HashMap<String, bool>,
    /// Per-module config blobs, every top-level key that isn't `modules` or
    /// `modules_dir` ends up here.
    pub config: HashMap<String, Value>,
    /// Optional override for the user modules directory.
    /// When set, this path is used *instead of* the default
    /// `<config_home>/modules`.  It does **not** affect the system modules dir.
    pub modules_dir_override: Option<PathBuf>,
    /// The raw mapping, kept for forward-compat / future use.
    pub extra: Mapping,
}

impl Config {
    pub fn from_value(v: Value) -> Result<Self, String> {
        let Some(root) = v.as_mapping() else {
            return Ok(Self::default());
        };

        let mut out = Self::default();

        // Parse `modules:` bool map.
        if let Some(modules_val) = root.get("modules")
            && let Some(m) = modules_val.as_mapping()
        {
            for (k, v) in m {
                let Some(name) = k.as_str() else {
                    return Err("modules key is not a string".into());
                };
                let Some(b) = v.as_bool() else {
                    return Err(format!("modules.{name} is not a bool"));
                };
                out.modules.insert(name.to_owned(), b);
            }
        }

        // Parse optional `modules_dir:` override.
        if let Some(dir_val) = root.get("modules_dir") {
            match dir_val.as_str() {
                Some(s) if !s.is_empty() => {
                    out.modules_dir_override = Some(PathBuf::from(s));
                }
                _ => {
                    return Err("modules_dir must be a non-empty string path".into());
                }
            }
        }

        // Everything else is per-module config.
        for (k, v) in root {
            let Some(key) = k.as_str() else { continue };
            if key == "modules" || key == "modules_dir" {
                continue;
            }
            out.config.insert(key.to_owned(), v.clone());
        }

        out.extra = root.clone();
        Ok(out)
    }

    #[inline]
    pub fn enabled(&self, name: &str) -> bool {
        self.modules.get(name).copied().unwrap_or(false)
    }

    #[inline]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.config.get(name)
    }
}

impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        self.modules == other.modules
            && self.config == other.config
            && self.modules_dir_override == other.modules_dir_override
    }
}

#[derive(Debug)]
pub enum ConfigEvent {
    Reload(Config),
    Err(Vec<String>),
}

pub struct ConfigInstruction {
    pub should_unrealize: bool,
    pub should_realize: bool,
    pub config_changed: bool,
}

pub fn compare_configs(old: &Config, new: &Config) -> HashMap<String, ConfigInstruction> {
    let mut names: HashSet<String> = old.modules.keys().cloned().collect();
    names.extend(new.modules.keys().cloned());
    names.extend(old.config.keys().cloned());
    names.extend(new.config.keys().cloned());

    let mut out = HashMap::new();
    for name in names {
        let old_enabled = old.enabled(&name);
        let new_enabled = new.enabled(&name);
        let old_cfg = old.get(&name);
        let new_cfg = new.get(&name);
        let config_changed = new_enabled && old_cfg != new_cfg;

        out.insert(
            name,
            ConfigInstruction {
                should_unrealize: old_enabled && !new_enabled,
                should_realize: !old_enabled && new_enabled,
                config_changed,
            },
        );
    }
    out
}

pub fn cfg_path(base: &Path) -> PathBuf {
    base.join("config.yaml")
}

/// Read and parse `<base>/config.yaml`.  Retries for up to 750 ms to
/// tolerate editors that write files non-atomically (same behaviour as the
/// original orbitd implementation).
pub fn load_cfg(base: &Path) -> Result<Config, String> {
    let path = cfg_path(base);
    let deadline = Instant::now() + Duration::from_millis(750);

    loop {
        match fs::read_to_string(&path) {
            Ok(text) => {
                return Config::from_value(
                    serde_yml::from_str(&text).map_err(|_| "invalid config.yaml")?,
                );
            }
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            // Config file absent → use defaults (all modules disabled).
            Err(_) => return Ok(Config::default()),
        }
    }
}
