use std::fmt;

use crossterm::event::Event;

use crate::config::LegitConfig;

pub enum Msg {
    TerminalEvent(Event),
    ConfigLoaded(LegitConfig),
    AuthTokenResolved(String),
    CommandFailed {
        context: &'static str,
        error: String,
    },
    Quit,
}

impl fmt::Debug for Msg {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TerminalEvent(event) => {
                formatter.debug_tuple("TerminalEvent").field(event).finish()
            }
            Self::ConfigLoaded(config) => {
                formatter.debug_tuple("ConfigLoaded").field(config).finish()
            }
            Self::AuthTokenResolved(_) => formatter
                .debug_tuple("AuthTokenResolved")
                .field(&"<redacted>")
                .finish(),
            Self::CommandFailed { context, error } => formatter
                .debug_struct("CommandFailed")
                .field("context", context)
                .field("error", error)
                .finish(),
            Self::Quit => formatter.write_str("Quit"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::msg::Msg;

    #[test]
    fn debug_redacts_auth_token() {
        let msg = Msg::AuthTokenResolved("secret-token".to_owned());

        let debug = format!("{msg:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
    }
}
