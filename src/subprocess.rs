//! Subprocess execution hardened for a raw-mode TUI.
//!
//! legit runs on the alternate screen with the terminal in raw mode and spawns
//! git/gh subprocesses on the blocking pool while the UI keeps drawing. Three
//! hazards come with that, all handled here — so every git/gh child is built
//! with [`git_command`]/[`gh_command`] and spawned through [`run_command`]
//! rather than a bare [`Command`]:
//!
//! * A child that stops to ask a question blocks reading the terminal we own.
//!   [`run_command`] nulls stdin and (on Unix) sheds the controlling terminal,
//!   while the builders set `GIT_TERMINAL_PROMPT=0`, so any prompt fails fast
//!   instead of hanging the UI.
//! * A `post-checkout` hook can background a process that inherits our captured
//!   stdout/stderr pipe; a plain `.output()` would then read to EOF forever.
//!   [`run_command`] bounds every call with a timeout and kills the process
//!   group to force the pipes closed.
//! * A child (and any hook-spawned `sudo`/`ssh`) outlives legit when the user
//!   quits mid-operation. Every child is tracked while it runs and
//!   [`terminate_all`] signals the whole group on shutdown.
//!
//! Splitting the concerns this way keeps the tracking sound: [`run_command`]
//! owns the process-group setup, so a child it spawns is *always* a group
//! leader and its PID is always a valid group id — there is no way to register a
//! child that can't then be signalled. The builders only carry git's own prompt
//! knob.
//!
//! The process-group machinery is Unix-only: [`run_command`] `setsid`s each
//! child into its own session, making it a group leader so its PID doubles as
//! its process-group id. Off Unix the timeout still applies but only the direct
//! child is signalled, and shutdown does not reap in-flight children.

#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    io::Read,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};

/// Hard ceiling on how long a single git/gh invocation may run before we treat
/// it as wedged and kill it. Generous enough for a slow `gh pr checkout` or a
/// legitimate post-checkout hook on a large repo, but finite so a hook that
/// never releases our output pipes (e.g. one that backgrounds a daemon) can't
/// hang the operation forever.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

/// After the child exits, how long to wait for a pipe reader to drain before
/// concluding a backgrounded descendant is still holding the write-end open and
/// killing the group to force EOF.
const DRAIN_GRACE: Duration = Duration::from_millis(500);

/// How often to check whether the child has exited while waiting on the timeout.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A `git` invocation that won't stop to prompt on the terminal the TUI owns.
///
/// `GIT_TERMINAL_PROMPT=0` turns git's own credential prompts into errors rather
/// than a blocking read of the terminal we're drawing over. This sets only the
/// command's *contents*; the stdin/session hardening is applied by
/// [`run_command`] when the command is spawned.
pub(crate) fn git_command() -> Command {
    let mut command = Command::new("git");
    command.env("GIT_TERMINAL_PROMPT", "0");
    command
}

/// A `gh` invocation hardened like [`git_command`] (gh shells out to git), plus
/// `GITHUB_TOKEN`/`GH_TOKEN` removed so gh reads its stored credentials rather
/// than an ambient token inherited from our environment.
pub(crate) fn gh_command() -> Command {
    let mut command = Command::new("gh");
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN");
    command
}

/// Extension trait that folds terminal-detachment into the [`Command`] builder
/// chain, next to `.stdin`/`.stdout`/`.stderr`.
trait DetachSessionExt {
    /// Put the spawned child in its own session, shedding the controlling
    /// terminal. [`run_command`] applies this to every child it spawns.
    ///
    /// legit runs on the alternate screen with the terminal in raw mode, so a
    /// child that reaches for `/dev/tty` — `sudo` or `ssh` fired from a
    /// `post-checkout` hook, say — would block reading the terminal we own. A
    /// fresh session has no controlling terminal, so those calls error out with
    /// "a terminal is required" instead of hanging on a prompt we can't answer.
    /// `setsid` also makes the child a session/process-group leader, which is
    /// what lets [`run_command`] and [`terminate_all`] signal the whole group
    /// (the hook and its descendants) by PID.
    ///
    /// A no-op off Unix, where there is no `setsid`/process-group model.
    fn detach_session(&mut self) -> &mut Self;
}

impl DetachSessionExt for Command {
    fn detach_session(&mut self) -> &mut Self {
        #[cfg(unix)]
        // SAFETY: the closure runs in the forked child before exec, where only
        // async-signal-safe calls are allowed. `setsid(2)` is async-signal-safe
        // and touches only the new child's session — no allocation, no shared
        // state.
        unsafe {
            self.pre_exec(|| {
                // A fresh session has no controlling terminal. EPERM (already a
                // group leader) can't happen for a just-forked child and would
                // only mean the tty is already shed, so ignoring it is safe.
                libc::setsid();
                Ok(())
            });
        }
        self
    }
}

/// Run `command`, returning its stdout on success.
///
/// The command is spawned (not `.output()`d) so the child can be tracked and
/// bounded: stdin is nulled and — on Unix — the child is `setsid`'d into its own
/// session, its process group is registered for [`terminate_all`], stdout/stderr
/// are drained on background threads, and the whole thing is capped at
/// [`COMMAND_TIMEOUT`]. Build `command` with [`git_command`]/[`gh_command`] so
/// `GIT_TERMINAL_PROMPT` is set as well.
pub(crate) fn run_command(label: &str, command: &mut Command) -> anyhow::Result<String> {
    run_with_timeout(label, command, COMMAND_TIMEOUT)
}

fn run_with_timeout(
    label: &str,
    command: &mut Command,
    timeout: Duration,
) -> anyhow::Result<String> {
    // Detach as part of the same builder chain, so a child we register is always
    // a group leader signal-able by its PID (see the module docs).
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .detach_session();

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run `{label}`"))?;

    #[cfg(unix)]
    let group = child.id() as i32;
    #[cfg(unix)]
    registry::register(group);

    // read_to_end blocks until every write-end of the pipe is closed, so drain
    // on separate threads; a hook that backgrounds a process keeps a write-end
    // open past the child's own exit, which is why `collect` may have to kill
    // the group to force EOF.
    let stdout_rx = spawn_reader(child.stdout.take());
    let stderr_rx = spawn_reader(child.stderr.take());

    let outcome = wait_with_timeout(&mut child, timeout);
    if outcome.is_none() {
        // Wedged past the budget: take the whole group down hard so the child
        // and any descendants die and release our pipes.
        signal_tree(&mut child, Signal::Kill);
    }

    let stdout = collect(&stdout_rx, &mut child);
    let stderr = collect(&stderr_rx, &mut child);
    // Reap the (now-dead) child so it doesn't linger as a zombie.
    let _ = child.wait();

    #[cfg(unix)]
    registry::unregister(group);

    let status = match outcome {
        Some(status) => status,
        None => bail!(
            "`{label}` timed out after {}s and was terminated",
            timeout.as_secs()
        ),
    };

    if !status.success() {
        let stderr = stderr_tail(&stderr);
        if stderr.is_empty() {
            bail!("`{label}` exited with {status}");
        }
        bail!("`{label}` failed: {stderr}");
    }

    String::from_utf8(stdout).with_context(|| format!("`{label}` returned non-utf8 output"))
}

/// Terminate every child (and its process group) still running when the UI
/// exits, so quitting mid-operation doesn't orphan git/gh — and any
/// hook-spawned `sudo`/`ssh` — to init. `SIGTERM` so a mid-flight `git worktree
/// add` can still drop its lock. Unix-only; a no-op elsewhere, where we don't
/// track process groups.
pub(crate) fn terminate_all() {
    #[cfg(unix)]
    terminate_groups(&registry::snapshot());
}

#[cfg(unix)]
fn terminate_groups(groups: &[i32]) {
    for &group in groups {
        signal_group(group, libc::SIGTERM);
    }
}

/// Spawn a thread that reads `pipe` to EOF and sends the bytes back. Returns a
/// receiver that yields exactly once (an empty vec if there was no pipe).
fn spawn_reader<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    match pipe {
        Some(mut pipe) => {
            thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        None => {
            let _ = tx.send(Vec::new());
        }
    }
    rx
}

/// Poll the child until it exits or the timeout elapses. `None` means it was
/// still running at the deadline (the caller kills it).
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            // Treat a wait error like a timeout: fall through to the kill path
            // rather than looping forever on a child we can't observe.
            Err(_) => return None,
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Take a reader's buffered output. If it doesn't drain within [`DRAIN_GRACE`],
/// a backgrounded descendant is holding the pipe open past the child's exit;
/// escalate `SIGTERM` -> `SIGKILL` on the group to force EOF, then take whatever
/// was read.
fn collect(rx: &mpsc::Receiver<Vec<u8>>, child: &mut Child) -> Vec<u8> {
    if let Ok(buf) = rx.recv_timeout(DRAIN_GRACE) {
        return buf;
    }
    signal_tree(child, Signal::Term);
    if let Ok(buf) = rx.recv_timeout(DRAIN_GRACE) {
        return buf;
    }
    signal_tree(child, Signal::Kill);
    // Bounded even after SIGKILL: a descendant that escaped the group (e.g. a
    // hook daemon that `setsid`s itself) never receives the group signal and can
    // hold the pipe open indefinitely. Give up on the drain and return what we
    // have — the reader thread leaks, but the operation doesn't wedge, which is
    // the whole point of the timeout.
    rx.recv_timeout(DRAIN_GRACE).unwrap_or_default()
}

enum Signal {
    Term,
    Kill,
}

/// Signal the child's whole process group on Unix (the child leads its own group
/// via `setsid`, so its descendants — the hook, a hook-spawned `sudo`/`ssh` — go
/// with it); fall back to signalling just the child elsewhere.
fn signal_tree(child: &mut Child, signal: Signal) {
    #[cfg(unix)]
    {
        let sig = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        signal_group(child.id() as i32, sig);
    }
    #[cfg(not(unix))]
    {
        // No process-group model without setsid; SIGKILL-equivalent only.
        let _ = signal;
        let _ = child.kill();
    }
}

/// Send `sig` to the process group led by `group` (via the negative-PID form of
/// `kill(2)`). The leader may already be reaped, but the group persists while
/// any descendant lives, and a fresh fork inherits its parent's group rather
/// than becoming leader of `group`, so PID reuse can't accidentally retarget an
/// unrelated process here.
#[cfg(unix)]
fn signal_group(group: i32, sig: i32) {
    if group > 1 {
        // SAFETY: kill(2) is a plain syscall with no memory effects.
        unsafe {
            libc::kill(-group, sig);
        }
    }
}

fn stderr_tail(stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_owned();
    const MAX: usize = 1_000;
    if stderr.chars().count() <= MAX {
        return stderr;
    }
    let suffix: String = stderr
        .chars()
        .rev()
        .take(MAX.saturating_sub(1))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{suffix}")
}

/// Live child process groups, tracked so [`terminate_all`] can signal whatever
/// is still running at shutdown. Keyed by process-group id (== the child's PID).
#[cfg(unix)]
mod registry {
    use std::{
        collections::HashSet,
        sync::{Mutex, OnceLock},
    };

    fn groups() -> &'static Mutex<HashSet<i32>> {
        static GROUPS: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
        GROUPS.get_or_init(|| Mutex::new(HashSet::new()))
    }

    pub(super) fn register(group: i32) {
        if let Ok(mut groups) = groups().lock() {
            groups.insert(group);
        }
    }

    pub(super) fn unregister(group: i32) {
        if let Ok(mut groups) = groups().lock() {
            groups.remove(&group);
        }
    }

    pub(super) fn snapshot() -> Vec<i32> {
        match groups().lock() {
            Ok(groups) => groups.iter().copied().collect(),
            Err(_) => Vec::new(),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Run `f` on a worker thread, returning `None` if it doesn't finish within
    /// `limit`. Every test below asserts on the returned value, so a regression
    /// that reintroduces a hang fails the test instead of wedging the suite.
    fn bounded<T: Send + 'static>(
        limit: Duration,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(limit).ok()
    }

    // `run_command`/`run_with_timeout` apply the stdin/session hardening
    // themselves, so a raw `sh -c` command is enough to exercise them.
    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn returns_stdout_on_success() {
        let out = bounded(Duration::from_secs(10), || {
            run_command("echo", &mut sh("echo hello"))
        })
        .expect("run_command should not hang")
        .expect("echo should succeed");
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn surfaces_failure_with_stderr() {
        let err = bounded(Duration::from_secs(10), || {
            run_command("boom", &mut sh("echo oops >&2; exit 3"))
        })
        .expect("run_command should not hang")
        .expect_err("non-zero exit should be an error");
        assert!(format!("{err:#}").contains("oops"), "got: {err:#}");
    }

    // #3: a post-checkout hook that backgrounds a process which inherits our
    // stdout pipe would keep `read_to_end` blocked past the child's own exit. The
    // child here exits immediately but leaves `sleep 30` holding the pipe; the
    // call must still return the child's output promptly rather than block for
    // 30s, by killing the group to force EOF.
    #[test]
    fn does_not_hang_when_a_backgrounded_process_holds_the_pipe() {
        let out = bounded(Duration::from_secs(10), || {
            run_command("bg", &mut sh("sleep 30 & echo done"))
        })
        .expect("a backgrounded pipe holder must not wedge the call")
        .expect("command should succeed");
        assert_eq!(out.trim(), "done");
    }

    // #3: a child that never exits must be killed at the deadline rather than
    // hang the operation forever.
    #[test]
    fn times_out_a_wedged_child() {
        let err = bounded(Duration::from_secs(10), || {
            run_with_timeout("wedged", &mut sh("sleep 600"), Duration::from_millis(200))
        })
        .expect("timeout path must not hang")
        .expect_err("a child past its deadline should be an error");
        assert!(format!("{err:#}").contains("timed out"), "got: {err:#}");
    }

    // #5: terminating a tracked group reaps the child (and, via its process
    // group, any descendants) so quitting mid-operation doesn't orphan it.
    #[test]
    fn terminating_a_group_reaps_the_child() {
        let mut command = Command::new("sleep");
        // Spawned directly (not via `run_command`), so detach it here to make it
        // a group leader — otherwise `terminate_groups` has no group to signal.
        command.arg("600").detach_session();
        let mut child = command.spawn().expect("spawn sleep");
        let group = child.id() as i32;

        terminate_groups(&[group]);

        let status = bounded(Duration::from_secs(10), move || child.wait().ok())
            .expect("wait should not hang after the group is terminated")
            .expect("child should be reaped");
        assert!(
            !status.success(),
            "a terminated child should not exit success"
        );
    }

    #[test]
    fn registry_tracks_and_forgets_groups() {
        // A value above Linux's max PID so it can never collide with a real
        // group; this test only exercises the set bookkeeping, never signals it.
        let sentinel = 12_345_678;
        registry::register(sentinel);
        assert!(registry::snapshot().contains(&sentinel));
        registry::unregister(sentinel);
        assert!(!registry::snapshot().contains(&sentinel));
    }
}
