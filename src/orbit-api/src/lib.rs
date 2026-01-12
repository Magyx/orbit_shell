use std::{fmt::Debug, sync::atomic::AtomicBool, time::Duration};

use serde::{Serialize, de::DeserializeOwned};
use ui::{sctk::SctkEvent, widget::Element};

pub use tracing;

#[allow(unused_imports)]
pub use self::macros::*;
pub use ui;
pub mod runtime;

mod macros;

#[derive(Debug)]
pub struct OrbitCtl {
    exit_orbit: AtomicBool,
    exit_module: AtomicBool,
}

impl Default for OrbitCtl {
    fn default() -> Self {
        Self::new()
    }
}

impl OrbitCtl {
    pub fn new() -> Self {
        Self {
            exit_orbit: AtomicBool::new(false),
            exit_module: AtomicBool::new(false),
        }
    }

    pub fn orbit_should_close(&self) -> bool {
        self.exit_orbit.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn module_should_close(&self) -> bool {
        self.exit_module
            .swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    pub fn close_orbit(&self) {
        self.exit_orbit
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn close_module(&self) {
        self.exit_module
            .store(true, std::sync::atomic::Ordering::Relaxed)
    }
}

#[derive(Debug)]
pub struct ErasedMsg {
    pub(crate) inner: Box<dyn crate::runtime::erased::DynMsg>,
}

pub type Engine<'a> = ui::graphics::Engine<'a, ErasedMsg>;

// TODO: add from stream/async
#[derive(Clone)]
pub enum Subscription<M: Send + 'static> {
    None,
    Batch(Vec<Subscription<M>>),
    Interval { every: Duration, message: M },
    Timeout { after: Duration, message: M },
}

pub type Event<M> = ui::event::Event<M, SctkEvent>;

pub trait OrbitModule: Default + 'static {
    type Config: Serialize + DeserializeOwned + Default;
    type Message: Send + Clone + 'static;

    fn cleanup<'a>(&mut self, _engine: &mut Engine<'a>);

    // Config
    fn validate_config_raw(cfg: &serde_yml::Value) -> Result<(), String> {
        _ = cfg;
        Ok(())
    }
    fn validate_config(cfg: Self::Config) -> Result<(), String> {
        _ = cfg;
        Ok(())
    }
    fn apply_config<'a>(
        &mut self,
        engine: &mut Engine<'a>,
        config: Self::Config,
        options: &mut ui::sctk::Options,
    ) -> bool {
        _ = engine;
        _ = config;
        _ = options;
        false
    }

    // UI
    fn update<'a>(
        &mut self,
        tid: ui::graphics::TargetId,
        engine: &mut Engine<'a>,
        event: &Event<Self::Message>,
        orbit: &OrbitCtl,
    ) -> bool {
        _ = tid;
        _ = engine;
        _ = event;
        _ = orbit;
        false
    }
    fn view(&self, tid: &ui::graphics::TargetId) -> Element<Self::Message>;

    fn subscriptions(&self) -> Subscription<Self::Message> {
        Subscription::None
    }
}
