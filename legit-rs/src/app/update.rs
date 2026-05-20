use crossterm::event::{Event, KeyCode, KeyEventKind};

use super::{cmd::Cmd, model::Model, msg::Msg};

pub fn update(model: &mut Model, msg: Msg) -> Vec<Cmd> {
    match msg {
        Msg::TerminalEvent(Event::Key(key)) => {
            if key.kind == KeyEventKind::Press && matches!(key.code, KeyCode::Char('q')) {
                model.should_quit = true;
            }
            Vec::new()
        }
        Msg::TerminalEvent(_) => Vec::new(),
        Msg::ConfigLoaded(config) => {
            model.config = config;
            model.last_error = None;
            Vec::new()
        }
        Msg::AuthTokenResolved(token) => {
            model.auth_token = Some(token);
            Vec::new()
        }
        Msg::CommandFailed { context, error } => {
            let message = format!("{context}: {error}");
            tracing::warn!(%message);
            model.last_error = Some(message);
            Vec::new()
        }
        Msg::Quit => {
            model.should_quit = true;
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent};

    use crate::app::{model::Model, msg::Msg, update::update};

    #[test]
    fn q_key_sets_should_quit() {
        let (mut model, _) = Model::new();

        update(
            &mut model,
            Msg::TerminalEvent(crossterm::event::Event::Key(KeyEvent::new(
                KeyCode::Char('q'),
                crossterm::event::KeyModifiers::NONE,
            ))),
        );

        assert!(model.should_quit);
    }
}
