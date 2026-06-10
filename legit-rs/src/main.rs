mod app;
mod auth;
mod blocker;
mod config;
mod file_category;
mod format;
mod git_remote;
mod github;
mod markdown;
mod runtime;
mod secret;
#[cfg(test)]
mod test_fixtures;
mod tracing_setup;
mod view;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let _tracing_guard = tracing_setup::init()?;
    tracing::info!("starting legit-rs");

    runtime::run().await
}
