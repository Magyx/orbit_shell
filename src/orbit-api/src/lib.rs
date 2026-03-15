use std::{fmt, pin::Pin, time::Duration};

use serde::{Serialize, de::DeserializeOwned};
use ui::{sctk::SctkEvent, widget::Element};

pub use tracing;

#[allow(unused_imports)]
pub use self::macros::*;
pub use ui;
pub mod runtime;
pub use serde;
pub use serde_yml;

mod macros;

pub type Event<M> = ui::event::Event<M, SctkEvent>;
pub type Engine<'a> = ui::graphics::Engine<'a, ErasedMsg>;

pub struct ErasedMsg {
    pub(crate) inner: Box<dyn crate::runtime::erased::DynMsg>,
}

impl fmt::Debug for ErasedMsg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ErasedMsg(..)")
    }
}

pub type BoxFuture<M> = Pin<Box<dyn Future<Output = M> + Send + 'static>>;

pub enum Task<M: Send + 'static> {
    None,
    Batch(Vec<Task<M>>),

    RedrawTarget,
    RedrawModule,
    ExitModule,
    ExitOrbit,

    Spawn(BoxFuture<M>),
}

impl<M: Send + 'static> Task<M> {
    pub fn batch(tasks: impl IntoIterator<Item = Task<M>>) -> Self {
        Self::Batch(tasks.into_iter().collect())
    }
    pub fn spawn<F>(fut: F) -> Self
    where
        F: Future<Output = M> + Send + 'static,
    {
        Self::Spawn(Box::pin(fut))
    }
}

// TODO: add from stream/async
#[derive(Clone)]
pub enum Subscription<M: Send + 'static> {
    None,
    Batch(Vec<Subscription<M>>),
    Interval { every: Duration, message: M },
    Timeout { after: Duration, message: M },
}

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
    ) -> Task<Self::Message> {
        _ = tid;
        _ = engine;
        _ = event;
        Task::None
    }
    fn view(&self, tid: &ui::graphics::TargetId) -> Element<Self::Message>;

    fn subscriptions(&self) -> Subscription<Self::Message> {
        Subscription::None
    }
}
