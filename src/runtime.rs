use std::{io, sync::Arc, thread};

use anyhow::{Context, Result};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event},
        execute,
        terminal::{
            DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen,
            disable_raw_mode, enable_raw_mode,
        },
    },
};
use tokio::sync::mpsc;

use crate::{
    app::{cmd, model::Model, msg::Msg, update::update},
    github::limiter::NetworkLimiter,
    view,
};

/// Hard ceiling on simultaneously in-flight GitHub HTTP requests across the
/// whole transport (all lanes). GitHub's documented secondary-rate-limit
/// ceiling is ~100 concurrent requests shared across REST + GraphQL, so 16
/// leaves ample headroom. See ADR 0003.
const MAX_CONCURRENT_REQUESTS: usize = 16;

/// Sub-cap on background-effective requests (the open-PR listing and the
/// enrichment fan-out); the remaining slots stay free for the focused PR's
/// fetches. Lane derivation and borrow semantics live in `github::limiter`.
const MAX_BACKGROUND_REQUESTS: usize = 8;

#[tracing::instrument(name = "tui_runtime", skip_all)]
pub async fn run() -> Result<()> {
    let _terminal_guard = TerminalGuard::enter()?;
    // Reap any git/gh subprocess still running when the UI exits (drops on every
    // return path, below the terminal guard), so quitting mid-worktree-creation
    // doesn't orphan it — and any hook-spawned sudo/ssh — to init.
    let _shutdown_sweep = crate::subprocess::ShutdownSweep::arm();
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;

    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    spawn_event_reader(event_tx);

    let limiter = NetworkLimiter::new(MAX_CONCURRENT_REQUESTS, MAX_BACKGROUND_REQUESTS);
    spawn_network_stats_forwarder(&limiter, &msg_tx);

    let (mut model, initial_cmds) = Model::new();
    tracing::info!(commands = initial_cmds.len(), "model initialized");
    spawn_cmds(initial_cmds, &msg_tx, &limiter);

    // Seed the viewport height before the first render so scroll math has the
    // right bounds even before the user resizes anything.
    let size = terminal.size().context("failed to query terminal size")?;
    process_msg(
        Msg::TerminalEvent(Event::Resize(size.width, size.height)),
        &mut model,
        &msg_tx,
        &limiter,
    );

    terminal.draw(|frame| view::view(&model, frame, chrono::Utc::now()))?;
    tracing::debug!("initial frame rendered");

    while !model.should_quit {
        let first_msg = tokio::select! {
            Some(event) = event_rx.recv() => Msg::TerminalEvent(event),
            Some(msg) = msg_rx.recv() => msg,
            else => Msg::Quit,
        };

        process_msg(first_msg, &mut model, &msg_tx, &limiter);

        while let Ok(event) = event_rx.try_recv() {
            process_msg(Msg::TerminalEvent(event), &mut model, &msg_tx, &limiter);
        }

        while let Ok(msg) = msg_rx.try_recv() {
            process_msg(msg, &mut model, &msg_tx, &limiter);
        }

        terminal.draw(|frame| view::view(&model, frame, chrono::Utc::now()))?;
        tracing::debug!(should_quit = model.should_quit, "frame rendered");
    }

    Ok(())
}

fn process_msg(
    msg: Msg,
    model: &mut Model,
    msg_tx: &mpsc::UnboundedSender<Msg>,
    limiter: &Arc<NetworkLimiter>,
) {
    tracing::debug!(?msg, "processing message");
    // The reducer's clock: the instant this message is processed. The Fetch Age
    // stamps record it. It is NOT the wall-clock the view reads — each draw
    // samples its own, later `chrono::Utc::now()` (lines 62/82), so the reducer
    // clock is just a past instant the view-now is measured against when
    // computing the displayed age.
    let cmds = update(model, msg, chrono::Utc::now());
    if !cmds.is_empty() {
        tracing::debug!(commands = cmds.len(), "update returned commands");
    }
    // Push the (possibly moved) focus to the limiter before the commands this
    // message produced can acquire, so a fetch for the newly-focused PR ranks
    // interactive from its first scheduling decision — and the previous PR's
    // pending fetches demote. A no-op when focus is unchanged.
    limiter.set_focus(model.focused_pr_key());
    spawn_cmds(cmds, msg_tx, limiter);
}

fn spawn_cmds(
    cmds: Vec<cmd::Cmd>,
    msg_tx: &mpsc::UnboundedSender<Msg>,
    limiter: &Arc<NetworkLimiter>,
) {
    for cmd in cmds {
        tracing::debug!(?cmd, "spawning command");
        let tx = msg_tx.clone();
        let limiter = Arc::clone(limiter);
        tokio::spawn(cmd::run(cmd, tx, limiter));
    }
}

/// Bridge the limiter's change stream into the model: every time the in-flight /
/// waiting counts move, deliver a `Msg::NetworkStatsChanged` so the status bar
/// re-renders. One long-lived task; ends when the channel closes at shutdown.
fn spawn_network_stats_forwarder(
    limiter: &Arc<NetworkLimiter>,
    msg_tx: &mpsc::UnboundedSender<Msg>,
) {
    let mut rx = limiter.subscribe();
    let tx = msg_tx.clone();
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let stats = *rx.borrow_and_update();
            if tx.send(Msg::NetworkStatsChanged(stats)).is_err() {
                break;
            }
        }
    });
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
        // Disable the terminal's auto-wrap (DECAWM). ratatui positions each row
        // with an absolute cursor move and never re-emits one mid-row, so if its
        // detected width ever disagrees with the real width (e.g. over
        // ssh+tmux at large sizes, ratatui#2167) the terminal would wrap each
        // full-width row and tile the list into columns. With auto-wrap off the
        // overflow is clipped instead of wrapped, so rows stay one-per-line.
        //
        // Capture the mouse so wheel ticks arrive as scroll events `update`
        // can route to the viewport. Without capture, terminals translate
        // wheel input in the alternate screen into arrow-key presses — which
        // are focus keys, so scrolling would drag the focused card along.
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            DisableLineWrap,
            EnableMouseCapture
        )
        .context("failed to enter alternate screen")?;
        guard.entered_alt_screen = true;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.entered_alt_screen {
            let _ = execute!(
                io::stdout(),
                DisableMouseCapture,
                EnableLineWrap,
                LeaveAlternateScreen
            );
        }
        let _ = disable_raw_mode();
        tracing::debug!("terminal restored");
    }
}
