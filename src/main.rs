mod app;
mod auth;
mod blocker;
mod canonical_path;
mod chip;
mod clipboard;
mod color;
mod config;
mod file_category;
mod format;
mod git_remote;
mod github;
mod markdown;
mod palette;
mod repo_slug;
mod runtime;
mod secret;
mod subprocess;
#[cfg(test)]
mod test_fixtures;
mod ticket;
mod tracing_setup;
mod view;
mod worktree;
mod wrap;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let _tracing_guard = tracing_setup::init()?;
    tracing::info!("starting legit");

    runtime::run().await
}
