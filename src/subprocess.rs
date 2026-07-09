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
//!   stdout/stderr; with piped capture a plain `.output()` would then read to
//!   EOF forever. Output is captured into anonymous temp files instead — a file
//!   read needs no EOF, so no inherited descriptor can wedge the capture —
//!   while [`run_command`] bounds the call with a timeout in case the child
//!   itself never exits, and sweeps any backgrounded straggler left in the
//!   child's process group once the child is gone.
//! * A child (and any hook-spawned `sudo`/`ssh`) outlives legit when the user
//!   quits mid-operation. Every child is tracked while it runs and the
//!   [`ShutdownSweep`] guard signals the whole group on shutdown, escalating
//!   `SIGTERM` -> `SIGKILL` for anything that ignores the first ask. The
//!   shutdown sweep also closes the registry, so a command task still in
//!   flight when the user quits can't spawn a child *behind* the sweep — a
//!   late spawn is killed on the spot and surfaces as an error.
//!
//! Splitting the concerns this way keeps the tracking sound: [`run_command`]
//! owns the process-group setup, so a child it spawns is *always* a group
//! leader and its PID is always a valid group id — there is no way to register a
//! child that can't then be signalled. The builders only carry the command's
//! *contents*: git's prompt/token knobs and the [`GitEnv`] repo-scoping policy.
//!
//! The process-group machinery is Unix-only: [`run_command`] `setsid`s each
//! child into its own session, making it a group leader so its PID doubles as
//! its process-group id. Off Unix the timeout still applies but only the direct
//! child is signalled, and shutdown does not reap in-flight children.

#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    ffi::OsStr,
    fs::File,
    io::{Read, Seek},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};

/// Hard ceiling on how long a single git/gh invocation may run before we treat
/// it as wedged and kill it. Generous enough for a slow `gh pr checkout` or a
/// legitimate post-checkout hook on a large repo, but finite so a hook that
/// never exits can't hang the operation forever.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(300);

/// Ceiling on the child-exit poll interval ([`wait_with_timeout`] backs off
/// exponentially up to this), and the cadence of the shutdown sweep's group
/// liveness poll.
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
}

impl std::ops::Deref for HardenedCommand {
    type Target = Command;

    fn deref(&self) -> &Command {
        &self.0
    }
}

/// Which git environment a command runs under — a required constructor
/// argument, so every call site decides the repo-scoping policy explicitly
/// rather than remembering an opt-in scrub.
///
/// Git exports `GIT_DIR`/`GIT_WORK_TREE`/etc. to hook subprocesses, and our
/// `.hooks/pre-push` hook runs `cargo test`. Those variables override `-C` and
/// `current_dir` when git locates the repository, so under [`GitEnv::Ambient`]
/// a command that drives git against a throwaway sandbox repo would instead
/// operate on the real repository the hook is running inside (appending stray
/// commits, flipping `core.bare`). Commands scoped to a path they are given —
/// the worktree operations — must use [`GitEnv::Scrubbed`]; a command aimed at
/// the user's real cwd repo (e.g. remote detection) *wants* [`GitEnv::Ambient`].
/// The policy applies to `gh` too, which shells out to `git`.
#[derive(Clone, Copy)]
pub(crate) enum GitEnv {
    /// Inherit the process's git environment untouched.
    Ambient,
    /// Strip the repo-locating variables so `-C`/`current_dir` alone decide
    /// which repository the command operates on. This fixed list is the only
    /// env mutation callers can express — deliberately not a general
    /// `env_remove`, which could unset the hardening the constructors applied.
    Scrubbed,
}

impl GitEnv {
    fn apply(self, command: &mut Command) {
        match self {
            Self::Ambient => {}
            Self::Scrubbed => {
                command
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_WORK_TREE")
                    .env_remove("GIT_INDEX_FILE")
                    .env_remove("GIT_OBJECT_DIRECTORY")
                    .env_remove("GIT_COMMON_DIR")
                    .env_remove("GIT_PREFIX");
            }
        }
    }
}

/// A `git` invocation that won't stop to prompt on the terminal the TUI owns,
/// running under the given [`GitEnv`].
///
/// `GIT_TERMINAL_PROMPT=0` turns git's own credential prompts into errors rather
/// than a blocking read of the terminal we're drawing over. This sets only the
/// command's *contents*; the stdin/session hardening is applied by
/// [`run_command`] when the command is spawned.
pub(crate) fn git_command(env: GitEnv) -> HardenedCommand {
    let mut command = Command::new("git");
    command.env("GIT_TERMINAL_PROMPT", "0");
    env.apply(&mut command);
    HardenedCommand(command)
}

/// A `gh` invocation hardened like [`git_command`] (gh shells out to git, so it
/// gets the same prompt knob and [`GitEnv`] policy), plus `GITHUB_TOKEN`/
/// `GH_TOKEN` removed so gh reads its stored credentials rather than an ambient
/// token inherited from our environment.
pub(crate) fn gh_command(env: GitEnv) -> HardenedCommand {
    let mut command = Command::new("gh");
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN");
    env.apply(&mut command);
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
    /// what lets [`run_command`] and the [`ShutdownSweep`] signal the whole
    /// group (the hook and its descendants) by PID.
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
/// session, its process group is registered for the [`ShutdownSweep`],
/// stdout/stderr are captured into anonymous temp files, and the whole thing is
/// capped at [`COMMAND_TIMEOUT`]. The [`HardenedCommand`] type guarantees
/// `command` came from [`git_command`]/[`gh_command`], so `GIT_TERMINAL_PROMPT`
/// is set as well.
pub(crate) fn run_command(label: &str, command: &mut HardenedCommand) -> anyhow::Result<String> {
    run_with_timeout(label, command, COMMAND_TIMEOUT)
}

fn run_with_timeout(
    label: &str,
    command: &mut HardenedCommand,
    timeout: Duration,
) -> anyhow::Result<String> {
    // Capture into anonymous temp files rather than pipes. A pipe only hits
    // EOF once every write-end closes, so a hook-backgrounded descendant that
    // inherits the child's stdio could hold a read open past the child's own
    // exit. A file read needs no EOF: once the child is gone we read whatever
    // it wrote, and an inherited descriptor held by a straggler costs nothing.
    let stdout_file = tempfile::tempfile().context("failed to create a capture file")?;
    let stderr_file = tempfile::tempfile().context("failed to create a capture file")?;

    // Reach the wrapped Command directly: spawning is this module's job, so the
    // stdio/session hardening lives here rather than on HardenedCommand's public
    // surface. Detach as part of the same builder chain, so a child we register
    // is always a group leader signal-able by its PID (see the module docs).
    let raw = &mut command.0;
    raw.stdin(Stdio::null())
        .stdout(Stdio::from(
            stdout_file
                .try_clone()
                .context("failed to clone a capture handle")?,
        ))
        .stderr(Stdio::from(
            stderr_file
                .try_clone()
                .context("failed to clone a capture handle")?,
        ))
        .detach_session();

    let mut child = raw
        .spawn()
        .with_context(|| format!("failed to run `{label}`"))?;

    // Tracked for the shutdown sweep while the guard lives; it unregisters on
    // every exit path below.
    #[cfg(unix)]
    let _registration = match registry::Registration::track(child.id() as i32) {
        Some(registration) => registration,
        None => {
            // The shutdown sweep already ran; this child spawned too late to be
            // in it and would outlive the TUI. Kill it before returning so
            // refusal keeps the sweep's guarantee instead of just reporting the
            // breach.
            signal_group(child.id() as i32, libc::SIGKILL);
            let _ = child.wait();
            bail!("`{label}` aborted: shutting down");
        }
    };

    let outcome = wait_with_timeout(&mut child, timeout);
    if outcome.is_none() {
        // Wedged past the budget: the same bounded TERM -> KILL escalation as
        // shutdown, so a mid-flight git command can drop its locks before the
        // group (the child and any descendants) is killed hard. The unreaped
        // child keeps the group probe alive, so even a TERM-compliant child
        // waits out the full grace here — a fixed cost on a call that already
        // blew a far larger budget.
        #[cfg(unix)]
        terminate_groups(&[child.id() as i32]);
        #[cfg(not(unix))]
        let _ = child.kill();
    }
    // Reap the child so it doesn't linger as a zombie (and so the group probe
    // below doesn't count it as a live member).
    let _ = child.wait();

    // A hook may have backgrounded a process into the child's group. Sweep it
    // now — the bounded TERM -> KILL escalation — so nothing the command
    // spawned outlives it (the registration above drops on return, after which
    // the shutdown sweep could no longer see the group). Probed first so the
    // common no-straggler case pays one signal-0 syscall, not a poll cycle.
    #[cfg(unix)]
    {
        let group = child.id() as i32;
        if group_alive(group) {
            terminate_groups(&[group]);
        }
    }

    let stdout = read_capture(stdout_file);
    let stderr = read_capture(stderr_file);

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

/// Guard that runs the shutdown sweep when dropped: terminates every child
/// (and its process group) still running when the UI exits, so quitting
/// mid-operation doesn't orphan git/gh — and any hook-spawned `sudo`/`ssh` —
/// to init. Arm one for the lifetime of the UI so the sweep runs on every exit
/// path. `SIGTERM` first so a mid-flight `git worktree add` can still drop its
/// lock, then `SIGKILL` after [`TERM_GRACE`] for anything that ignored it.
/// Blocks up to the grace period only while a child is actually still dying.
/// Closes the registry first, so a command task racing the sweep can't spawn a
/// child behind it — [`run_command`] refuses (and kills) any spawn that lands
/// after the close. Unix-only in effect; a no-op elsewhere, where we don't
/// track process groups.
///
/// Arm and drop bound one *generation* of the registry: [`arm`](Self::arm)
/// (re)opens it, drop closes it. Closing is therefore not process-permanent —
/// a later runtime invocation in the same process arms its own sweep and
/// tracks afresh instead of failing every spawn as "shutting down". A
/// stale-generation spawn that lands after a reopen is simply tracked — and
/// swept — by the new generation, so the last sweep to drop still sees every
/// live group.
pub(crate) struct ShutdownSweep;

impl ShutdownSweep {
    pub(crate) fn arm() -> Self {
        #[cfg(unix)]
        registry::open();
        Self
    }
}

impl Drop for ShutdownSweep {
    fn drop(&mut self) {
        #[cfg(unix)]
        terminate_groups(&registry::close());
    }
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

/// Everything the child (and anything that inherited its stdio) wrote to
/// `file` so far. Read errors degrade to whatever was readable — the child's
/// exit status, not the capture, decides whether the command succeeded.
fn read_capture(mut file: File) -> Vec<u8> {
    let mut buf = Vec::new();
    if file.rewind().is_ok() {
        let _ = file.read_to_end(&mut buf);
    }
    buf
}

/// Poll the child until it exits or the timeout elapses. `None` means it was
/// still running at the deadline (the caller kills it). The poll backs off
/// exponentially from 1ms to [`POLL_INTERVAL`], so a short-lived command is
/// noticed within a few milliseconds without spinning on a long-running one.
fn wait_with_timeout(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    let mut interval = Duration::from_millis(1);
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
        thread::sleep(interval);
        interval = (interval * 2).min(POLL_INTERVAL);
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

/// Live child process groups, tracked so the [`super::ShutdownSweep`] can
/// signal whatever is still running at shutdown. Keyed by process-group id
/// (== the child's PID).
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

        /// Accept registrations again: the start of a new [`ShutdownSweep`]
        /// generation after a previous sweep closed the registry. Anything
        /// registered from here on belongs to — and is swept by — the new
        /// generation.
        ///
        /// [`ShutdownSweep`]: super::ShutdownSweep
        pub(super) fn open(&mut self) {
            self.closed = false;
        }
    }

    /// The global registry, with lock poisoning recovered rather than papered
    /// over: the critical sections are trivial `HashSet`/flag updates that
    /// can't leave the `Registry` logically inconsistent, while any fallback
    /// would silently lose tracking — an untracked child survives the
    /// shutdown sweep, and a poisoned `close` would make the sweep reap
    /// nothing at all.
    fn lock() -> MutexGuard<'static, Registry> {
        static GLOBAL: OnceLock<Mutex<Registry>> = OnceLock::new();
        GLOBAL
            .get_or_init(|| Mutex::new(Registry::default()))
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// RAII tracking of one child's process group in the global registry:
    /// registered while the guard lives, unregistered on drop — structurally,
    /// on every exit path, so no early return can leak an entry for the
    /// shutdown sweep to signal after the group id has been recycled.
    pub(super) struct Registration(i32);

    impl Registration {
        /// Track `group`. `None` once the registry is closed: the shutdown
        /// sweep has already signalled everything it could see, so the caller
        /// must kill the child itself rather than let it run untracked.
        pub(super) fn track(group: i32) -> Option<Self> {
            lock().register(group).then_some(Self(group))
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            lock().unregister(self.0);
        }
    }

    pub(super) fn close() -> Vec<i32> {
        lock().close()
    }

    pub(super) fn open() {
        lock().open();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    // Every test below asserts on `bounded`'s returned value, so a regression
    // that reintroduces a hang fails the test instead of wedging the suite.
    use crate::test_fixtures::bounded;

    // `run_command`/`run_with_timeout` apply the stdin/session hardening
    // themselves, so a raw `sh -c` command (wrapped through the test-only
    // escape hatch) is enough to exercise them.
    fn sh(script: &str) -> HardenedCommand {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        HardenedCommand::raw(command)
    }

    // The constructors' half of the hardening contract (the spawn-time half —
    // stdin/session/timeout/tracking — is exercised by the tests below):
    // neither builder may let git stop to prompt on the terminal the TUI owns,
    // and gh must read its stored token rather than an ambient one.
    #[test]
    fn constructors_disable_prompts_and_gh_drops_ambient_tokens() {
        let disables_prompt = |label: &str, command: &Command| {
            assert!(
                command.get_envs().any(|(key, value)| {
                    key == OsStr::new("GIT_TERMINAL_PROMPT") && value == Some(OsStr::new("0"))
                }),
                "{label} should set GIT_TERMINAL_PROMPT=0"
            );
        };
        disables_prompt("git_command", &git_command(GitEnv::Ambient));
        disables_prompt("gh_command", &gh_command(GitEnv::Ambient));

        let gh = gh_command(GitEnv::Ambient);
        for token in ["GITHUB_TOKEN", "GH_TOKEN"] {
            assert!(
                gh.get_envs()
                    .any(|(key, value)| key == OsStr::new(token) && value.is_none()),
                "gh_command should drop ambient {token}"
            );
        }
    }

    const SCRUBBED_VARS: [&str; 6] = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
    ];

    // The repo-scoping half of the constructor contract: GitEnv::Scrubbed must
    // mark every repo-locating variable for removal so a sandboxed call can't
    // be redirected onto the repo a hook is running inside — for direct `git`
    // calls and for `gh`, which shells out to git — while GitEnv::Ambient must
    // leave them alone so cwd-repo operations keep working under a hook.
    #[test]
    fn git_env_policy_scrubs_or_preserves_the_repo_locating_variables() {
        let marks_removed = |command: &Command, var: &str| {
            command
                .get_envs()
                .any(|(key, value)| key == OsStr::new(var) && value.is_none())
        };

        for (label, command) in [
            ("git_command", git_command(GitEnv::Scrubbed)),
            ("gh_command", gh_command(GitEnv::Scrubbed)),
        ] {
            for var in SCRUBBED_VARS {
                assert!(
                    marks_removed(&command, var),
                    "scrubbed {label} should mark {var} for removal"
                );
            }
        }

        for (label, command) in [
            ("git_command", git_command(GitEnv::Ambient)),
            ("gh_command", gh_command(GitEnv::Ambient)),
        ] {
            for var in SCRUBBED_VARS {
                assert!(
                    !marks_removed(&command, var),
                    "ambient {label} should not touch {var}"
                );
            }
        }
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
    // stdio would, under piped capture, block the read past the child's own
    // exit. The child here exits immediately but leaves `sleep 30` holding its
    // stdio: the call must return the child's output promptly (a capture file
    // needs no EOF), and the straggler sweep must kill the backgrounded
    // process rather than let it outlive the command that spawned it.
    #[test]
    fn returns_promptly_and_sweeps_a_backgrounded_stdio_holder() {
        let out = bounded(Duration::from_secs(10), || {
            run_command("bg", &mut sh("sleep 30 & echo $!"))
        })
        .expect("a backgrounded stdio holder must not wedge the call")
        .expect("command should succeed");
        let straggler: i32 = out
            .trim()
            .parse()
            .expect("child should print the backgrounded pid");

        // The sweep signals before run_command returns, but init may not have
        // reaped the orphan yet, so poll briefly for it to vanish.
        let deadline = Instant::now() + Duration::from_secs(5);
        // SAFETY: kill(2) with the null signal only probes for existence.
        while unsafe { libc::kill(straggler, 0) == 0 } {
            assert!(
                Instant::now() < deadline,
                "the straggler sweep should have killed the backgrounded process"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    // The escaped-descendant edge: a process that `setsid`s itself out of the
    // child's group never receives the group signals, so it survives holding a
    // capture-file descriptor indefinitely. That must cost nothing — the
    // child's own output still comes back promptly.
    #[test]
    fn returns_output_when_an_escaped_descendant_holds_the_capture_file() {
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
        .expect("an escaped capture-file holder must not wedge the call")
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

        // Reopening starts a new generation: registration works again, and the
        // new generation's close sees what was registered under it.
        registry.open();
        assert!(
            registry.register(10),
            "registration after reopen must succeed"
        );
        assert_eq!(registry.close(), vec![10]);
    }
}
