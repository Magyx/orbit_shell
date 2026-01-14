use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use calloop::channel as loop_channel;
use serde_yml::{Mapping, Value};

#[derive(Debug)]
pub enum ConfigEvent {
    Reload(Config),
    Err(Vec<String>),
}

#[derive(Default, Debug)]
pub struct Config {
    pub modules: HashMap<String, bool>,
    pub config: HashMap<String, Value>,
    pub extra: Mapping,
}

impl Config {
    pub fn from_value(v: Value) -> Result<Self, String> {
        let Some(root) = v.as_mapping() else {
            return Ok(Self::default());
        };

        let mut out = Self::default();

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

        for (k, v) in root {
            let Some(key) = k.as_str() else { continue };

            if key == "modules" {
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
        self.modules == other.modules && self.config == other.config
    }
}

pub struct ConfigWatcher {
    tx: loop_channel::Sender<ConfigEvent>,
    handle: Option<JoinHandle<()>>,
    stop_tx: Option<mpsc::Sender<()>>,
}

impl ConfigWatcher {
    pub fn new() -> (loop_channel::Channel<ConfigEvent>, Self) {
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

    // TODO: need to watch potential location.
    pub fn start(&mut self, base: &Path) {
        assert!(
            self.handle.is_none(),
            "orbitd config watcher already started"
        );
        tracing::debug!(base = %base.display(), "starting config watcher");

        let (stop_tx, stop_rx) = mpsc::channel();
        self.stop_tx = Some(stop_tx);

        let tx = self.tx.clone();

        let base = base.to_path_buf();
        let cfg_path = cfg_path(&base);
        let cfg_name = cfg_path.file_name().map(|s| s.to_os_string());

        let handle = std::thread::spawn(move || {
            use notify::{
                Config as NConfig, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
                event::ModifyKind,
            };

            let (n_tx, n_rx) = std::sync::mpsc::channel();
            let mut watcher = match RecommendedWatcher::new(n_tx, NConfig::default()) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!(error = ?e, "failed to create notify watcher");
                    return;
                }
            };
            if let Err(e) = watcher.watch(&base, RecursiveMode::NonRecursive) {
                tracing::error!(error = ?e, "failed to watch config dir");
                return;
            };

            let mut last = Instant::now() - Duration::from_millis(500);
            let debounce = Duration::from_millis(150);

            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }

                match n_rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(Ok(ev)) => {
                        let touches_cfg =
                            ev.paths.iter().any(|p| match (&cfg_name, p.file_name()) {
                                (Some(want), Some(got)) => want == got,
                                _ => false,
                            });
                        if !touches_cfg {
                            continue;
                        }

                        let reloadish = matches!(
                            ev.kind,
                            EventKind::Modify(ModifyKind::Data(_))
                                | EventKind::Modify(ModifyKind::Name(_))
                                | EventKind::Create(_)
                                | EventKind::Remove(_)
                                | EventKind::Modify(ModifyKind::Any)
                        );

                        if reloadish && last.elapsed() >= debounce {
                            tracing::info!("config changed, reloading");
                            let event = match load_cfg(&base) {
                                Ok(v) => ConfigEvent::Reload(v),
                                Err(e) => ConfigEvent::Err(vec![e]),
                            };
                            let _ = tx.send(event);
                            last = Instant::now();
                        }
                    }
                    Ok(Err(_)) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
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

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn xdg_config_home() -> PathBuf {
    let base = xdg::BaseDirectories::new().config_home.unwrap_or_default();
    base.join("orbit")
}

pub fn modules_dir_if_exists(config_path: &Path) -> Option<PathBuf> {
    let dir = config_path.join("modules");
    if dir.is_dir() { Some(dir) } else { None }
}

pub fn cfg_path(base: &Path) -> PathBuf {
    base.join("config.yaml")
}

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
            Err(_) => return Ok(Config::default()),
        }
    }
}

pub struct ConfigInstruction {
    pub should_unrealize: bool,
    pub should_realize: bool,
    pub config_changed: bool,
}

pub fn compare_configs(
    old: &Config,
    new: &Config,
) -> Result<HashMap<String, ConfigInstruction>, &'static str> {
    let mut names: HashSet<String> = old.modules.keys().cloned().collect();
    names.extend(new.modules.keys().cloned());
    names.extend(old.config.keys().cloned());
    names.extend(new.config.keys().cloned());

    let mut out = HashMap::new();
    for name in names {
        let old_enabled = old.enabled(&name);
        let new_enabled = new.enabled(&name);

        let old_cfg = old.get(name.as_str());
        let new_cfg = new.get(name.as_str());
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

    Ok(out)
}
