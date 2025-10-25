use std::{fmt::Debug, sync::atomic::AtomicBool, time::Duration};

use serde::{Serialize, de::DeserializeOwned};
use ui::{
    graphics::{Engine, TargetId},
    sctk::SctkEvent,
    widget::Element,
};

#[allow(unused_imports)]
pub use self::macros::*;
pub use ui;
pub mod runtime;

mod macros;

// TODO: need to add a way for the modules to signal they want to be closed
#[derive(Debug)]
pub struct OrbitLoop {
    exit: AtomicBool,
    // tx: calloop::channel::Channel<>
}

impl Default for OrbitLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl OrbitLoop {
    pub fn new() -> Self {
        Self {
            exit: AtomicBool::new(false),
        }
    }

    pub fn should_close(&self) -> bool {
        self.exit.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn close(&self) {
        self.exit.store(true, std::sync::atomic::Ordering::Relaxed);
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

    fn cleanup<'a>(&mut self, _engine: &mut Engine<'a, ErasedMsg>);

    // Config
    fn init_config(config: &mut serde_yml::Value) {
        let merged: Self::Config = serde_yml::from_value(config.clone()).unwrap_or_default();
        *config = serde_yml::to_value(merged).expect("serialize merged config");
    }
    fn validate_config(config: &serde_yml::Value) -> Result<(), String> {
        serde_yml::from_value::<Self::Config>(config.clone())
            .map(|_: Self::Config| ())
            .map_err(|e| e.to_string())
    }
    fn config_updated<'a>(
        &mut self,
        _engine: &mut Engine<'a, ErasedMsg>,
        _config: &serde_yml::Value,
    ) {
    }

    // UI
    fn update<'a>(
        &mut self,
        _tid: TargetId,
        _engine: &mut Engine<'a, ErasedMsg>,
        _event: &Event<Self::Message>,
        _orbit: &OrbitLoop,
    ) -> bool {
        false
    }
    fn view(&self, _tid: &TargetId) -> Element<Self::Message>;

    fn subscriptions(&self) -> Subscription<Self::Message> {
        Subscription::None
    }
}
