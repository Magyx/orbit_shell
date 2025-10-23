use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use calloop::channel as loop_channel;

#[derive(Debug)]
pub enum ConfigEvent {
    Reload(serde_yml::Value),
    Err(String),
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

    pub fn start(&mut self, base: &Path) {
        assert!(
            self.handle.is_none(),
            "orbitd config watcher already started"
        );

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
            let mut watcher = RecommendedWatcher::new(n_tx, NConfig::default()).unwrap();

            watcher.watch(&base, RecursiveMode::NonRecursive).unwrap();

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
                            let event = match load_cfg(&base) {
                                Ok(v) => ConfigEvent::Reload(v),
                                Err(e) => ConfigEvent::Err(e.into()),
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

pub fn ensure_exists(base: &PathBuf) -> Result<(), &'static str> {
    fs::create_dir_all(base).map_err(|_| "failed to create config dir")?;
    fs::create_dir_all(modules_dir(base)).map_err(|_| "failed to create modules dir")?;

    let config = cfg_path(base);
    if !config.exists() {
        fs::write(&config, "modules: {}\n").map_err(|_| "failed to init config.yaml")?;
    }
    Ok(())
}

pub fn xdg_config_home() -> PathBuf {
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut home = env::var_os("HOME")
                .map(PathBuf::from)
                .expect("HOME not set");
            home.push(".config");
            home
        });

    base.join("orbit")
}

pub fn modules_dir(base: &Path) -> PathBuf {
    base.join("modules")
}

pub fn cfg_path(base: &Path) -> PathBuf {
    base.join("config.yaml")
}

pub fn load_cfg(base: &Path) -> Result<serde_yml::Value, &'static str> {
    let path = cfg_path(base);
    let deadline = Instant::now() + Duration::from_millis(750);

    loop {
        match fs::read_to_string(&path) {
            Ok(text) => return serde_yml::from_str(&text).map_err(|_| "invalid config.yaml"),
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(_) => return Err("failed to read config.yaml"),
        }
    }
}

pub fn store_cfg(base: &Path, cfg: &serde_yml::Value) -> Result<(), &'static str> {
    let s = serde_yml::to_string(cfg).map_err(|_| "failed to serialize config.yaml")?;
    fs::write(cfg_path(base), s).map_err(|_| "failed to write config.yaml")
}
