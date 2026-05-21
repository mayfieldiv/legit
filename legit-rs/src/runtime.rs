use std::{io, thread};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;

use crate::{
    app::{cmd, model::Model, msg::Msg, update::update},
    view,
};

#[tracing::instrument(name = "tui_runtime", skip_all)]
pub async fn run() -> Result<()> {
    let _terminal_guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    spawn_event_reader(event_tx);

    let (mut model, initial_cmds) = Model::new();
    tracing::info!(commands = initial_cmds.len(), "model initialized");
    spawn_cmds(initial_cmds, &msg_tx);
    terminal.draw(|frame| view::view(&model, frame, chrono::Utc::now()))?;
    tracing::debug!("initial frame rendered");

    while !model.should_quit {
        let first_msg = tokio::select! {
            Some(event) = event_rx.recv() => Msg::TerminalEvent(event),
            Some(msg) = msg_rx.recv() => msg,
            else => Msg::Quit,
        };

        process_msg(first_msg, &mut model, &msg_tx);

        while let Ok(event) = event_rx.try_recv() {
            process_msg(Msg::TerminalEvent(event), &mut model, &msg_tx);
        }

        while let Ok(msg) = msg_rx.try_recv() {
            process_msg(msg, &mut model, &msg_tx);
        }

        terminal.draw(|frame| view::view(&model, frame, chrono::Utc::now()))?;
        tracing::debug!(should_quit = model.should_quit, "frame rendered");
    }

    Ok(())
}

fn process_msg(msg: Msg, model: &mut Model, msg_tx: &mpsc::UnboundedSender<Msg>) {
    tracing::debug!(?msg, "processing message");
    let cmds = update(model, msg);
    if !cmds.is_empty() {
        tracing::debug!(commands = cmds.len(), "update returned commands");
    }
    spawn_cmds(cmds, msg_tx);
}

fn spawn_cmds(cmds: Vec<cmd::Cmd>, msg_tx: &mpsc::UnboundedSender<Msg>) {
    for cmd in cmds {
        tracing::debug!(?cmd, "spawning command");
        let tx = msg_tx.clone();
        tokio::spawn(cmd::run(cmd, tx));
    }
}

fn spawn_event_reader(event_tx: mpsc::UnboundedSender<Event>) {
    tracing::debug!("spawning terminal event reader");
    thread::spawn(move || {
        loop {
            match event::read() {
                Ok(event) => {
                    if event_tx.send(event).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to read terminal event");
                    break;
                }
            }
        }
    });
}

struct TerminalGuard {
    entered_alt_screen: bool,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        tracing::debug!("enabling raw mode");
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut guard = Self {
            entered_alt_screen: false,
        };
        tracing::debug!("entering alternate screen");
        execute!(io::stdout(), EnterAlternateScreen).context("failed to enter alternate screen")?;
        guard.entered_alt_screen = true;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.entered_alt_screen {
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
        }
        let _ = disable_raw_mode();
        tracing::debug!("terminal restored");
    }
}
