mod app;
mod auth;
mod blocker;
mod config;
mod format;
mod git_remote;
mod github;
mod runtime;
mod secret;
mod tracing_setup;
mod view;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let _tracing_guard = tracing_setup::init()?;
    tracing::info!("starting legit-rs");

    runtime::run().await
}
