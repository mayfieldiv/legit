use crossterm::event::Event;

use crate::config::LegitConfig;

#[derive(Debug)]
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
