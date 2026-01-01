use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init() {
    let _ = tracing_log::LogTracer::init();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .try_init()
        .ok();

    std::panic::set_hook(Box::new(|panic| {
        tracing::error!(panic = ?panic, "panic");
    }));
}
