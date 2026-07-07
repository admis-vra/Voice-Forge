//! Logging setup: writes rolling daily log files under `<data_dir>/logs` and also
//! mirrors output to stderr in debug builds. Uses the `tracing` ecosystem.

use anyhow::Result;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::config::Config;

/// Initializes logging. The returned [`WorkerGuard`] must be kept alive for the
/// lifetime of the program — dropping it flushes and stops the background writer.
pub fn init() -> Result<WorkerGuard> {
    let log_dir = Config::data_dir()?.join("logs");
    std::fs::create_dir_all(&log_dir)?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "voiceforge.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Default to `info`; override with the RUST_LOG env var.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,voiceforge=debug"));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(non_blocking);

    let registry = tracing_subscriber::registry().with(filter).with(file_layer);

    // Also log to stderr while developing.
    #[cfg(debug_assertions)]
    let registry = registry.with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

    registry.init();

    Ok(guard)
}
