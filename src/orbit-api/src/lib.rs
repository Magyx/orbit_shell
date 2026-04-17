use std::{fmt, pin::Pin, sync::Arc, time::Duration};

use serde::{Serialize, de::DeserializeOwned};
use ui::{sctk::SctkEvent, widget::Element};

pub use orbit_macros::orbit_config;
pub use tracing;
pub use ui;
#[doc(hidden)]
pub mod runtime;
#[doc(hidden)]
pub use schemars;
#[doc(hidden)]
pub use serde;
#[doc(hidden)]
pub use serde_json;
#[doc(hidden)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    Disconnected,
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SubscriptionSender: failed to send message (receiver dropped)"
        )
    }
}

impl std::error::Error for SendError {}

pub struct SubscriptionSender<M> {
    pub(crate) inner: Arc<dyn Fn(M) -> Result<(), SendError> + Send + Sync + 'static>,
}

impl<M: Send + 'static> SubscriptionSender<M> {
    #[doc(hidden)]
    pub fn new(f: Arc<dyn Fn(M) -> Result<(), SendError> + Send + Sync + 'static>) -> Self {
        Self { inner: f }
    }

    pub fn send(&self, msg: M) -> Result<(), SendError> {
        (self.inner)(msg)
    }
}

pub type BoxStreamFactory<M> =
    Box<dyn FnOnce(SubscriptionSender<M>) -> BoxFuture<()> + Send + 'static>;

pub enum Subscription<M: Send + 'static> {
    None,
    Batch(Vec<Subscription<M>>),
    Interval { every: Duration, message: M },
    Timeout { after: Duration, message: M },
    SyncedInterval { every: Duration, message: M },
    SyncedTimeout { after: Duration, message: M },
    Stream(BoxStreamFactory<M>),
}

impl<M: Send + 'static> Subscription<M> {
    pub fn stream<F, Fut>(f: F) -> Self
    where
        F: FnOnce(SubscriptionSender<M>) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        Self::Stream(Box::new(|tx| Box::pin(f(tx))))
    }
}

impl<M: Send + Clone + 'static> Clone for Subscription<M> {
    fn clone(&self) -> Self {
        match self {
            Self::None => Self::None,
            Self::Batch(v) => Self::Batch(v.clone()),
            Self::Interval { every, message } => Self::Interval {
                every: *every,
                message: message.clone(),
            },
            Self::Timeout { after, message } => Self::Timeout {
                after: *after,
                message: message.clone(),
            },
            Self::SyncedInterval { every, message } => Self::SyncedInterval {
                every: *every,
                message: message.clone(),
            },
            Self::SyncedTimeout { after, message } => Self::SyncedTimeout {
                after: *after,
                message: message.clone(),
            },
            // FnOnce factories are non-clonable; degrade gracefully.
            Self::Stream(_) => Self::None,
        }
    }
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
