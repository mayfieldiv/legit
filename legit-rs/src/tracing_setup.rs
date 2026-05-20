use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, fmt::format::FmtSpan};

pub fn init() -> Result<WorkerGuard> {
    let log_dir = log_dir()?;
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create {}", log_dir.display()))?;

    let file_appender = tracing_appender::rolling::never(log_dir, "legit-rs.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_new(env::var("LEGIT_LOG").unwrap_or_else(|_| "info".to_owned()))
        .context("invalid LEGIT_LOG filter")?;

    fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .init();
    Ok(guard)
}

fn log_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".legit/log"))
}
