use ratatui::crossterm::event::Event;

use crate::{
    config::LegitConfig, git_remote::RepoInfo, github::limiter::NetworkStats, github::rest::PR,
    secret::Secret,
};

#[derive(Debug)]
pub enum Msg {
    TerminalEvent(Event),
    ConfigLoaded(LegitConfig),
    AuthTokenResolved(Secret<String>),
    RepoDetected(RepoInfo),
    PrArrived(PR),
    PrListLoaded,
    NetworkStatsChanged(NetworkStats),
    PrListFailed {
        context: &'static str,
        error: String,
    },
    CommandFailed {
        context: &'static str,
        error: String,
    },
    Quit,
}

#[cfg(test)]
mod tests {
    use crate::{app::msg::Msg, secret::Secret};

    #[test]
    fn debug_redacts_auth_token() {
        let msg = Msg::AuthTokenResolved(Secret::new("secret-token".to_owned()));

        let debug = format!("{msg:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
    }
}
