use std::{path::Path, sync::OnceLock};

use tracing::level_filters::LevelFilter;
use tracing_subscriber::{EnvFilter, fmt, prelude::*, util::SubscriberInitExt};

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

pub fn init(base: &Path) {
    let _ = tracing_log::LogTracer::init();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let file_appender = tracing_appender::rolling::daily(base.join("logs/"), "orbit.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    LOG_GUARD.set(guard).ok();

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_filter(filter))
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(file_writer)
                .with_filter(LevelFilter::DEBUG),
        )
        .try_init()
        .ok();

    std::panic::set_hook(Box::new(|panic| {
        tracing::error!(panic = ?panic, "panic");
    }));
}
