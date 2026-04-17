use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crate::config::{ConfigEvent, load_cfg};

/// Watches `<base>/config.yaml` for changes and invokes a caller-supplied
/// callback with a [`ConfigEvent`] whenever the file is modified.
///
/// The callback runs on the watcher's background thread, so it must be
/// `Send`.  Callers that need to bridge to a specific event loop (e.g.
/// `calloop`) should forward through a channel inside the callback — this
/// keeps the watcher itself loop-agnostic.
///
/// # Example — bridging to calloop (orbitd style)
/// ```ignore
/// let (tx, rx) = calloop::channel::channel::<ConfigEvent>();
/// let mut watcher = ConfigWatcher::new(base, move |ev| { let _ = tx.send(ev); });
/// watcher.start();
/// ```
///
/// # Example — bridging to std mpsc (orbit-schema style)
/// ```ignore
/// let (tx, rx) = std::sync::mpsc::channel::<ConfigEvent>();
/// let mut watcher = ConfigWatcher::new(base, move |ev| { let _ = tx.send(ev); });
/// watcher.start();
/// ```
pub struct ConfigWatcher {
    base: PathBuf,
    callback: Box<dyn FnMut(ConfigEvent) + Send + 'static>,
    handle: Option<JoinHandle<()>>,
    stop_tx: Option<mpsc::Sender<()>>,
}

// TODO: need to watch potential location.
impl ConfigWatcher {
    pub fn new<F>(base: &Path, callback: F) -> Self
    where
        F: FnMut(ConfigEvent) + Send + 'static,
    {
        Self {
            base: base.to_path_buf(),
            callback: Box::new(callback),
            handle: None,
            stop_tx: None,
        }
    }

    /// Start watching.  Panics if called a second time without an intervening
    /// [`stop`](Self::stop).
    pub fn start(&mut self) {
        assert!(
            self.handle.is_none(),
            "ConfigWatcher::start called while already running"
        );
        tracing::debug!(base = %self.base.display(), "starting config watcher");

        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        self.stop_tx = Some(stop_tx);

        // We need to move the callback into the thread.  Take it out and put a
        // no-op placeholder back — `start` may only be called once per
        // lifetime so this is fine.
        let mut callback = std::mem::replace(&mut self.callback, Box::new(|_| {}));

        let base = self.base.clone();
        let cfg_path = crate::config::cfg_path(&base);
        let cfg_name = cfg_path.file_name().map(|s| s.to_os_string());

        let handle = std::thread::Builder::new()
            .name("orbit-config-watcher".into())
            .spawn(move || {
                use notify::{
                    Config as NConfig, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
                    event::ModifyKind,
                };

                let (n_tx, n_rx) = mpsc::channel();
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
                }

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
                                callback(event);
                                last = Instant::now();
                            }
                        }
                        Ok(Err(_)) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("failed to spawn config watcher thread");

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
