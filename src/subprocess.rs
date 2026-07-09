//! Subprocess execution hardened for a raw-mode TUI.
//!
//! legit runs on the alternate screen with the terminal in raw mode and spawns
//! git/gh subprocesses on the blocking pool while the UI keeps drawing. Three
//! hazards come with that, all handled here — so every git/gh child is built
//! with [`git_command`]/[`gh_command`] (the only production constructors of
//! [`HardenedCommand`], the type [`run_command`] spawns) rather than a bare
//! [`Command`]:
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
//!   [`terminate_all`] signals the whole group on shutdown, escalating
//!   `SIGTERM` -> `SIGKILL` for anything that ignores the first ask. The
//!   shutdown sweep also closes the registry, so a command task still in
//!   flight when the user quits can't spawn a child *behind* the sweep — a
//!   late spawn is killed on the spot and surfaces as an error.
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
    ffi::OsStr,
    io::{ErrorKind, Read},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, mpsc},
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

/// How long shutdown gives SIGTERM'd children to exit — long enough for a
/// mid-flight `git worktree add` to drop its lock — before SIGKILLing the
/// survivors so nothing outlives the TUI.
const TERM_GRACE: Duration = Duration::from_secs(1);

/// A [`Command`] built by this module's hardened constructors.
///
/// [`git_command`]/[`gh_command`] are the only production ways to obtain one,
/// and the wrapped [`Command`] stays private: the mutable surface is limited to
/// the builder methods below, and only [`run_command`] can spawn it. So a git/gh
/// child that skipped the prompt/token hardening — or the stdin/session/timeout
/// /tracking hardening applied at spawn — is unrepresentable rather than a
/// convention callers must remember; there is no `.spawn()`/`.output()` to reach
/// and no way to unset what the constructors set. Derefs immutably to
/// [`Command`] for read-only inspection (every spawning or mutating `Command`
/// method takes `&mut self`, so the shared reference exposes no bypass).
pub(crate) struct HardenedCommand(Command);

impl HardenedCommand {
    /// Wrap a raw [`Command`], bypassing the hardened constructors — for tests
    /// that need to drive arbitrary scripts through [`run_command`].
    #[cfg(test)]
    fn raw(command: Command) -> Self {
        Self(command)
    }

    pub(crate) fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.0.arg(arg);
        self
    }

    pub(crate) fn args(&mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> &mut Self {
        self.0.args(args);
        self
    }

    pub(crate) fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.0.current_dir(dir);
        self
    }

    /// Strip the ambient git environment from the command.
    ///
    /// Git exports `GIT_DIR`/`GIT_WORK_TREE`/etc. to hook subprocesses, and our
    /// `.hooks/pre-push` hook runs `cargo test`. Those variables override `-C`
    /// and `current_dir` when git locates the repository, so without stripping
    /// them a command that drives git against a throwaway sandbox repo would
    /// instead operate on the real repository the hook is running inside
    /// (appending stray commits, flipping `core.bare`). Applies both to direct
    /// `git` calls and to `gh`, which shells out to `git` — both must be
    /// scrubbed. Opt-in, because a command aimed at the user's real cwd repo
    /// (e.g. remote detection) *wants* the ambient environment.
    ///
    /// This fixed list is the only env mutation callers get — deliberately not
    /// a general `env_remove`, which could unset the hardening the constructors
    /// applied.
    pub(crate) fn scrub_git_env(&mut self) -> &mut Self {
        self.0
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_PREFIX");
        self
    }
}

impl std::ops::Deref for HardenedCommand {
    type Target = Command;

    fn deref(&self) -> &Command {
        &self.0
    }
}

/// A `git` invocation that won't stop to prompt on the terminal the TUI owns.
///
/// `GIT_TERMINAL_PROMPT=0` turns git's own credential prompts into errors rather
/// than a blocking read of the terminal we're drawing over. This sets only the
/// command's *contents*; the stdin/session hardening is applied by
/// [`run_command`] when the command is spawned.
pub(crate) fn git_command() -> HardenedCommand {
    let mut command = Command::new("git");
    command.env("GIT_TERMINAL_PROMPT", "0");
    HardenedCommand(command)
}

/// A `gh` invocation hardened like [`git_command`] (gh shells out to git), plus
/// `GITHUB_TOKEN`/`GH_TOKEN` removed so gh reads its stored credentials rather
/// than an ambient token inherited from our environment.
pub(crate) fn gh_command() -> HardenedCommand {
    let mut command = Command::new("gh");
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN");
    HardenedCommand(command)
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
                // group leader) can't happen for a just-forked child — POSIX
                // guarantees a fork child's PID matches no active process-group
                // id — but the whole tracking scheme rests on the child being a
                // group leader, so surface a failure as a spawn error rather
                // than trusting that reasoning.
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
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
/// [`COMMAND_TIMEOUT`]. The [`HardenedCommand`] type guarantees `command` came
/// from [`git_command`]/[`gh_command`], so `GIT_TERMINAL_PROMPT` is set as well.
pub(crate) fn run_command(label: &str, command: &mut HardenedCommand) -> anyhow::Result<String> {
    run_with_timeout(label, command, COMMAND_TIMEOUT)
}

fn run_with_timeout(
    label: &str,
    command: &mut HardenedCommand,
    timeout: Duration,
) -> anyhow::Result<String> {
    // Reach the wrapped Command directly: spawning is this module's job, so the
    // stdio/session hardening lives here rather than on HardenedCommand's public
    // surface. Detach as part of the same builder chain, so a child we register
    // is always a group leader signal-able by its PID (see the module docs).
    let raw = &mut command.0;
    raw.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .detach_session();

    let mut child = raw
        .spawn()
        .with_context(|| format!("failed to run `{label}`"))?;

    #[cfg(unix)]
    let group = child.id() as i32;
    #[cfg(unix)]
    if !registry::register(group) {
        // The shutdown sweep already ran; this child spawned too late to be in
        // it and would outlive the TUI. Kill it before returning so refusal
        // keeps the sweep's guarantee instead of just reporting the breach.
        signal_group(group, libc::SIGKILL);
        let _ = child.wait();
        bail!("`{label}` aborted: shutting down");
    }

    // A pipe only hits EOF once every write-end is closed, so drain on separate
    // threads; a hook that backgrounds a process keeps a write-end open past
    // the child's own exit, which is why `collect` may have to kill the group
    // to force EOF.
    let stdout_capture = spawn_reader(child.stdout.take());
    let stderr_capture = spawn_reader(child.stderr.take());

    let outcome = wait_with_timeout(&mut child, timeout);
    if outcome.is_none() {
        // Wedged past the budget: take the whole group down hard so the child
        // and any descendants die and release our pipes.
        signal_tree(&mut child, Signal::Kill);
    }

    let stdout = collect(&stdout_capture, &mut child);
    let stderr = collect(&stderr_capture, &mut child);
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
/// hook-spawned `sudo`/`ssh` — to init. `SIGTERM` first so a mid-flight `git
/// worktree add` can still drop its lock, then `SIGKILL` after [`TERM_GRACE`]
/// for anything that ignored it. Blocks up to the grace period only while a
/// child is actually still dying. Closes the registry first, so a command task
/// racing this sweep can't spawn a child behind it — [`run_command`] refuses
/// (and kills) any spawn that lands after the close. Unix-only; a no-op
/// elsewhere, where we don't track process groups.
pub(crate) fn terminate_all() {
    #[cfg(unix)]
    terminate_groups(&registry::close());
}

#[cfg(unix)]
fn terminate_groups(groups: &[i32]) {
    for &group in groups {
        signal_group(group, libc::SIGTERM);
    }

    // Bounded escalation: a descendant that ignores SIGTERM (a hook-spawned
    // daemon, say) would otherwise survive quit. Poll group liveness through
    // the grace period, then SIGKILL whatever remains.
    let deadline = Instant::now() + TERM_GRACE;
    let mut alive: Vec<i32> = groups.to_vec();
    while !alive.is_empty() && Instant::now() < deadline {
        thread::sleep(POLL_INTERVAL);
        alive.retain(|&group| group_alive(group));
    }
    for &group in &alive {
        signal_group(group, libc::SIGKILL);
    }
}

/// Whether the process group led by `group` still has members, probed with the
/// null signal. An exited child its `run_command` thread hasn't reaped yet
/// counts as alive; the worst that costs is waiting out the grace and sending
/// a harmless SIGKILL to a group of zombies.
#[cfg(unix)]
fn group_alive(group: i32) -> bool {
    if group <= 1 {
        return false;
    }
    // SAFETY: kill(2) with the null signal only probes for existence.
    unsafe { libc::kill(-group, 0) == 0 }
}

/// One pipe's output, filled by a background reader thread.
///
/// The reader appends into the shared buffer as bytes arrive rather than
/// handing everything over in one message at EOF — that is what lets
/// [`collect`] salvage the output already read when EOF never comes (a
/// descendant that escaped the process group holding the write-end open).
struct PipeCapture {
    buf: Arc<Mutex<Vec<u8>>>,
    eof: mpsc::Receiver<()>,
}

impl PipeCapture {
    /// Whether the reader reached EOF within `grace`.
    fn drained(&self, grace: Duration) -> bool {
        self.eof.recv_timeout(grace).is_ok()
    }

    /// The bytes read so far.
    fn take(&self) -> Vec<u8> {
        self.buf
            .lock()
            .map(|mut buf| std::mem::take(&mut *buf))
            .unwrap_or_default()
    }
}

/// Spawn a thread that reads `pipe` until EOF (or a read error), appending into
/// the returned capture's buffer as bytes arrive. The capture reports EOF
/// immediately if there was no pipe.
fn spawn_reader<R: Read + Send + 'static>(pipe: Option<R>) -> PipeCapture {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let (tx, eof) = mpsc::channel();
    match pipe {
        Some(mut pipe) => {
            let sink = Arc::clone(&buf);
            thread::spawn(move || {
                let mut chunk = [0u8; 8 * 1024];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut sink) = sink.lock() {
                                sink.extend_from_slice(&chunk[..n]);
                            }
                        }
                        Err(error) if error.kind() == ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                let _ = tx.send(());
            });
        }
        None => {
            let _ = tx.send(());
        }
    }
    PipeCapture { buf, eof }
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
fn collect(capture: &PipeCapture, child: &mut Child) -> Vec<u8> {
    if capture.drained(DRAIN_GRACE) {
        return capture.take();
    }
    signal_tree(child, Signal::Term);
    if capture.drained(DRAIN_GRACE) {
        return capture.take();
    }
    signal_tree(child, Signal::Kill);
    // Bounded even after SIGKILL: a descendant that escaped the group (e.g. a
    // hook daemon that `setsid`s itself) never receives the group signal and can
    // hold the pipe open indefinitely. Give up on the drain and take what the
    // reader has buffered so far — the reader thread leaks, but the operation
    // doesn't wedge, which is the whole point of the timeout.
    let _ = capture.drained(DRAIN_GRACE);
    capture.take()
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
        sync::{Mutex, MutexGuard, OnceLock, PoisonError},
    };

    /// The state behind the module's global. A plain struct so the close /
    /// late-register handshake can be unit-tested on an owned instance —
    /// closing the *global* in a test would refuse spawns for every other
    /// test sharing the process.
    #[derive(Default)]
    pub(super) struct Registry {
        groups: HashSet<i32>,
        closed: bool,
    }

    impl Registry {
        /// Track `group`. Returns `false` once the registry is closed: the
        /// shutdown sweep has already signalled everything it could see, so
        /// the caller must kill the child itself rather than let it run
        /// untracked.
        pub(super) fn register(&mut self, group: i32) -> bool {
            if self.closed {
                return false;
            }
            self.groups.insert(group);
            true
        }

        pub(super) fn unregister(&mut self, group: i32) {
            self.groups.remove(&group);
        }

        /// Stop accepting registrations and drain the tracked groups. Because
        /// `register` refuses from this point on (and refused callers kill
        /// their own child), the returned snapshot is complete — no spawn can
        /// slip in behind the shutdown sweep.
        pub(super) fn close(&mut self) -> Vec<i32> {
            self.closed = true;
            self.groups.drain().collect()
        }
    }

    /// The global registry, with lock poisoning recovered rather than papered
    /// over: the critical sections are trivial `HashSet`/flag updates that
    /// can't leave the `Registry` logically inconsistent, while any fallback
    /// would silently lose tracking — an untracked child survives
    /// [`super::terminate_all`], and a poisoned `close` would make the
    /// shutdown sweep reap nothing at all.
    fn lock() -> MutexGuard<'static, Registry> {
        static GLOBAL: OnceLock<Mutex<Registry>> = OnceLock::new();
        GLOBAL
            .get_or_init(|| Mutex::new(Registry::default()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn register(group: i32) -> bool {
        lock().register(group)
    }

    pub(super) fn unregister(group: i32) {
        lock().unregister(group);
    }

    pub(super) fn close() -> Vec<i32> {
        lock().close()
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
    // themselves, so a raw `sh -c` command (wrapped through the test-only
    // escape hatch) is enough to exercise them.
    fn sh(script: &str) -> HardenedCommand {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        HardenedCommand::raw(command)
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

    // The escaped-descendant edge: a process that `setsid`s itself out of the
    // child's group never receives the group kill, so it can hold our pipe open
    // indefinitely. The child's own output must still come back — buffered
    // bytes must not be dropped along with the wedged pipe.
    #[test]
    fn returns_buffered_output_when_an_escaped_descendant_holds_the_pipe() {
        // util-linux setsid(1); absent on e.g. macOS, where this edge can't be
        // modelled from a shell one-liner.
        let setsid_available = Command::new("setsid")
            .arg("true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !setsid_available {
            eprintln!("skipping: setsid(1) not available");
            return;
        }

        let out = bounded(Duration::from_secs(10), || {
            run_command("escape", &mut sh("echo trapped; setsid -f sleep 30"))
        })
        .expect("an escaped pipe holder must not wedge the call")
        .expect("command should succeed");
        assert_eq!(out.trim(), "trapped");
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

    // #4 (shutdown escalation): a child that ignores SIGTERM must still be gone
    // after `terminate_groups` returns, via the SIGKILL escalation.
    #[test]
    fn terminating_a_group_kills_a_sigterm_ignoring_child() {
        // `trap '' TERM` marks SIGTERM ignored, and ignored dispositions
        // survive exec(2), so the `sleep` runs immune to the first, polite
        // signal. `echo ready` is read back before signalling so the test
        // can't race the trap being installed (which would let plain SIGTERM
        // win and leave the escalation unexercised).
        let mut command = sh("trap '' TERM; echo ready; exec sleep 600");
        let raw = &mut command.0;
        raw.stdout(Stdio::piped()).detach_session();
        let mut child = raw.spawn().expect("spawn trap child");
        let mut ready = [0u8; 6];
        child
            .stdout
            .take()
            .expect("stdout piped")
            .read_exact(&mut ready)
            .expect("child should report ready");
        let group = child.id() as i32;

        terminate_groups(&[group]);

        let status = bounded(Duration::from_secs(10), move || child.wait().ok())
            .expect("wait should not hang after the group is killed")
            .expect("child should be reaped");
        assert!(
            !status.success(),
            "a SIGKILLed child should not exit success"
        );
    }

    // Exercised on an owned instance, not the global — closing the global
    // would refuse spawns for every other test in this process. The group ids
    // are pure bookkeeping here; nothing is signalled.
    #[test]
    fn registry_drains_on_close_and_refuses_late_registration() {
        let mut registry = registry::Registry::default();
        assert!(registry.register(7), "registration while open must succeed");
        assert!(registry.register(8));
        registry.unregister(8);

        assert_eq!(registry.close(), vec![7]);

        assert!(
            !registry.register(9),
            "a group registered after close must be refused"
        );
        assert!(
            registry.close().is_empty(),
            "a refused registration must not be tracked"
        );
    }
}
