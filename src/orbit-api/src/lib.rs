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

    fn cleanup<'a>(&mut self, _engine: &mut ui::graphics::Engine<'a, ErasedMsg>);

    // Config
    fn validate_config(config: &serde_yml::Value) -> Result<(), String> {
        serde_yml::from_value::<Self::Config>(config.clone())
            .map(|_: Self::Config| ())
            .map_err(|e| e.to_string())
    }
    fn apply_config<'a>(
        &mut self,
        _engine: &mut ui::graphics::Engine<'a, ErasedMsg>,
        _config: Self::Config,
        _options: &mut ui::sctk::Options,
    ) -> bool {
        false
    }

    // UI
    fn update<'a>(
        &mut self,
        _tid: ui::graphics::TargetId,
        _engine: &mut ui::graphics::Engine<'a, ErasedMsg>,
        _event: &Event<Self::Message>,
        _orbit: &OrbitCtl,
    ) -> bool {
        false
    }
    fn view(&self, _tid: &ui::graphics::TargetId) -> Element<Self::Message>;

    fn subscriptions(&self) -> Subscription<Self::Message> {
        Subscription::None
    }
}
