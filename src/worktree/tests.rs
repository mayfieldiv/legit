use std::{
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;
use crate::{
    config::RepoConfig,
    github::rest::PrKey,
    github::types::PRState,
    test_fixtures::{bounded, fixed_created_at},
};

fn sample_pr(head_ref: &str, head_owner: &str) -> PR {
    PR {
        number: 42,
        repo_slug: "acme/widgets".to_owned(),
        title: "patch".to_owned(),
        author: "octocat".to_owned(),
        created_at: fixed_created_at(),
        updated_at: fixed_created_at(),
        additions: 0,
        deletions: 0,
        is_draft: false,
        labels: Vec::new(),
        requested_reviewers: Vec::new(),
        assignees: Vec::new(),
        review_decision: String::new(),
        mergeable: "UNKNOWN".to_owned(),
        last_commit_date: None,
        head_commit_sha: None,
        review_status_loaded: false,
        head_ref: head_ref.to_owned(),
        base_ref: "main".to_owned(),
        head_repository_owner: head_owner.to_owned(),
        state: PRState::Open,
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("legit-rs-{name}-{nanos}"))
}

fn run_git(args: &[&str], cwd: &Path) -> String {
    // Drive the fixture git through the same hardened spawn path as production
    // rather than a raw `.output()`, so the test helper can't be the one place
    // that bypasses HardenedCommand's guarantees.
    let mut command = git_command();
    command.args(args).current_dir(cwd);
    run_command("git", &mut command)
        .unwrap_or_else(|error| panic!("git {} failed: {error:#}", args.join(" ")))
}

/// A source repo under `root` with one (empty) commit.
fn init_source_repo(root: &Path) -> PathBuf {
    let source = root.join("source");
    fs::create_dir_all(&source).expect("create source repo");
    run_git(&["init"], &source);
    run_git(
        &[
            "-c",
            "user.name=Legit Test",
            "-c",
            "user.email=legit@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
        &source,
    );
    source
}

#[test]
fn parses_a_single_attached_worktree_on_a_branch() {
    let stdout = [
        "worktree /Users/me/src/widgets",
        "HEAD abc123def4567890abc123def4567890abc123de",
        "branch refs/heads/main",
        "",
    ]
    .join("\n");

    assert_eq!(
        parse_worktree_list(&stdout),
        vec![WorktreeEntry {
            path: "/Users/me/src/widgets".to_owned(),
            head: "abc123def4567890abc123def4567890abc123de".to_owned(),
            branch_ref: Some("refs/heads/main".to_owned()),
            branch_name: Some("main".to_owned()),
            detached: false,
            bare: false,
            locked: None,
            prunable: None,
        }]
    );
}

#[test]
fn parses_multiple_worktrees_with_flags() {
    let stdout = [
        "worktree /Users/me/src/widgets",
        "bare",
        "",
        "worktree /Users/me/.legit/worktrees/acme/widgets/1-foo",
        "HEAD deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        "branch refs/heads/foo",
        "",
        "worktree /tmp/detached-head",
        "HEAD aabbccddaabbccddaabbccddaabbccddaabbccdd",
        "detached",
        "locked because I said so",
        "",
        "worktree /tmp/orphan",
        "HEAD 0000000000000000000000000000000000000000",
        "detached",
        "prunable gitdir file points to non-existent location",
        "",
    ]
    .join("\n");

    let entries = parse_worktree_list(&stdout);

    assert_eq!(entries.len(), 4);
    assert!(entries[0].bare);
    assert_eq!(entries[1].branch_name.as_deref(), Some("foo"));
    assert!(entries[2].detached);
    assert_eq!(entries[2].locked.as_deref(), Some("because I said so"));
    assert_eq!(
        entries[3].prunable.as_deref(),
        Some("gitdir file points to non-existent location")
    );
}

#[test]
fn parser_ignores_empty_records_and_tolerates_bare_locked() {
    let stdout = "\n\nworktree /a\nHEAD 1111111111111111111111111111111111111111\nbranch refs/heads/x\nlocked\n\n\n\n";

    let entries = parse_worktree_list(stdout);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].locked.as_deref(), Some(""));
}

#[test]
fn sanitizes_branch_for_path() {
    assert_eq!(sanitize_branch_for_path("feature/login"), "feature-login");
    assert_eq!(sanitize_branch_for_path("feat: cool!"), "feat-cool");
    assert_eq!(sanitize_branch_for_path("a//b"), "a-b");
    assert_eq!(
        sanitize_branch_for_path("release.v1_beta"),
        "release.v1_beta"
    );
    assert_eq!(sanitize_branch_for_path("/foo/"), "foo");
    assert_eq!(sanitize_branch_for_path(&"a".repeat(200)).len(), 80);
}

#[test]
fn expected_branch_for_pr_matches_gh_checkout_convention() {
    assert_eq!(
        expected_branch_for_pr(&sample_pr("feature/foo", "acme"), "acme"),
        "feature/foo"
    );
    assert_eq!(
        expected_branch_for_pr(&sample_pr("patch-1", "contributor"), "acme"),
        "contributor-patch-1"
    );
    assert_eq!(
        expected_branch_for_pr(&sample_pr("patch-1", ""), "acme"),
        "patch-1"
    );
}

#[test]
fn matches_by_branch_before_path() {
    let entries = vec![
        WorktreeEntry {
            path: "/some/other/place".to_owned(),
            head: "a".repeat(40),
            branch_ref: Some("refs/heads/unrelated".to_owned()),
            branch_name: Some("unrelated".to_owned()),
            detached: false,
            bare: false,
            locked: None,
            prunable: None,
        },
        WorktreeEntry {
            path: "/Users/me/.legit/worktrees/acme/widgets/1-foo".to_owned(),
            head: "b".repeat(40),
            branch_ref: Some("refs/heads/foo".to_owned()),
            branch_name: Some("foo".to_owned()),
            detached: false,
            bare: false,
            locked: None,
            prunable: None,
        },
    ];

    assert_eq!(
        match_worktree(&entries, "foo", "/nonmatching/path").map(|entry| entry.path.as_str()),
        Some("/Users/me/.legit/worktrees/acme/widgets/1-foo")
    );
    assert_eq!(
        match_worktree(
            &entries,
            "ghost-branch",
            "/Users/me/.legit/worktrees/acme/widgets/1-foo",
        )
        .and_then(|entry| entry.branch_name.as_deref()),
        Some("foo")
    );
    assert!(match_worktree(&entries, "ghost", "/nowhere").is_none());
}

#[test]
fn resolves_worktree_paths_from_config() {
    let config = LegitConfig {
        repos: vec![RepoConfig {
            slug: "acme/widgets".to_owned(),
            ..Default::default()
        }],
        ..Default::default()
    };

    assert_eq!(
        resolve_worktree_path(&config, "acme/widgets", 1234, "feature/foo")
            .expect("default worktree path"),
        home_dir()
            .expect("home directory")
            .join(".legit/worktrees/acme/widgets/1234-feature-foo")
    );

    let config = LegitConfig {
        repos: vec![RepoConfig {
            slug: "acme/widgets".to_owned(),
            worktree_root: Some("/wts/widgets".to_owned()),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(
        resolve_worktree_path(&config, "acme/widgets", 7, "main")
            .expect("repo-specific worktree path"),
        PathBuf::from("/wts/widgets/7-main")
    );

    let config = LegitConfig {
        repos: vec![RepoConfig {
            slug: "acme/widgets".to_owned(),
            ..Default::default()
        }],
        worktree_root: Some("/srv/wts".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        resolve_worktree_path(&config, "acme/widgets", 7, "main").expect("global worktree path"),
        PathBuf::from("/srv/wts/acme/widgets/7-main")
    );
}

#[test]
fn resolves_source_clone_from_config() {
    let config = LegitConfig {
        repos: vec![
            RepoConfig {
                slug: "acme/widgets".to_owned(),
                source_clone: Some("~/src/widgets".to_owned()),
                ..Default::default()
            },
            RepoConfig {
                slug: "acme/gadgets".to_owned(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    assert_eq!(
        resolve_source_clone(&config, "acme/widgets").expect("sourceClone path"),
        Some(home_dir().expect("home directory").join("src/widgets"))
    );
    assert_eq!(
        resolve_source_clone(&config, "acme/gadgets").expect("missing sourceClone"),
        None
    );
    assert_eq!(
        resolve_source_clone(&config, "acme/unknown").expect("unknown repo sourceClone"),
        None
    );
}

#[test]
fn home_expansion_requires_home() {
    let error = home_dir_from(None).expect_err("missing HOME should fail");

    assert_eq!(error.to_string(), "HOME is not set");
}

#[test]
fn empty_home_is_treated_as_missing() {
    let error = home_dir_from(Some(OsString::new())).expect_err("empty HOME should fail");

    assert_eq!(error.to_string(), "HOME is not set");
}

#[test]
fn tilde_config_paths_require_home() {
    let error = resolve_config_path_with(
        "~/src/widgets",
        || anyhow::bail!("HOME is not set"),
        || Ok(PathBuf::from("/cwd")),
    )
    .expect_err("tilde path without HOME should fail");

    assert!(error.to_string().contains("HOME is not set"));
}

#[test]
fn relative_config_paths_require_current_dir() {
    let error = resolve_config_path_with(
        "src/widgets",
        || Ok(PathBuf::from("/home/me")),
        || Err(io::Error::new(io::ErrorKind::NotFound, "cwd was deleted")),
    )
    .expect_err("relative path without current dir should fail");

    assert!(
        error
            .to_string()
            .contains("failed to resolve current directory")
    );
}

#[test]
fn checkout_failure_removes_partial_worktree() {
    let root = temp_dir("checkout-cleanup");
    let source = init_source_repo(&root);
    let target = root.join("worktree");

    let error = create_worktree_for_pr_with_checkout(&source, &target, 42, |_target, _number| {
        anyhow::bail!("checkout failed")
    })
    .expect_err("checkout failure should be returned");

    assert!(
        format!("{error:#}").contains("checkout failed"),
        "original checkout error should be preserved: {error:#}"
    );
    assert!(
        !target.exists(),
        "partial worktree directory should be removed"
    );
    let worktrees = run_git(&["worktree", "list", "--porcelain"], &source);
    assert!(
        !worktrees.contains(&target.to_string_lossy().to_string()),
        "partial worktree should be unregistered: {worktrees}"
    );

    let _ = fs::remove_dir_all(root);
}

/// A source repo under `root` with one commit and the given executable
/// `post-checkout` hook script.
#[cfg(unix)]
fn source_repo_with_post_checkout_hook(root: &Path, hook_script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let source = init_source_repo(root);

    let hook = source.join(".git/hooks/post-checkout");
    fs::create_dir_all(hook.parent().expect("hook parent")).expect("create hooks dir");
    fs::write(&hook, hook_script).expect("write hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod hook");
    source
}

/// Create a worktree with a checkout that mirrors `gh pr checkout` — a real
/// `git checkout` in the target, which fires the repo's `post-checkout`
/// hook — under [`bounded`], so a regression that blocks on the hook surfaces
/// as a failed assertion rather than a hung suite. (The suite still exits
/// promptly on that failure: the child's stdin is nulled and `run_command`
/// enforces its own timeout.)
#[cfg(unix)]
fn create_worktree_bounded(source: &Path, target: &Path) -> anyhow::Result<()> {
    let source = source.to_path_buf();
    let target = target.to_path_buf();
    bounded(std::time::Duration::from_secs(30), move || {
        create_worktree_for_pr_with_checkout(&source, &target, 42, |target, _number| {
            let mut checkout = git_command();
            checkout
                .arg("-C")
                .arg(target)
                .args(["checkout", "--detach", "HEAD"]);
            run_command("git checkout", &mut checkout).map(|_| ())
        })
    })
    .expect("worktree creation should not block on the hook")
}

// A `post-checkout` hook that runs `sudo` (or git/ssh asking for a
// credential) would block reading the terminal the TUI owns and hang the
// whole app. `run_command` nulls the child's stdin so such a read gets EOF
// instead of blocking. Model that with a hook that reads stdin before doing
// its work: with the fix it proceeds and leaves a marker; without it, this
// would hang. (`worktree add` runs with --no-checkout, so the hook fires
// during the checkout step, as it would under `gh pr checkout`.)
#[cfg(unix)]
#[test]
fn worktree_creation_does_not_block_on_a_hook_reading_stdin() {
    let root = temp_dir("hook-stdin");
    let target = root.join("worktree");
    let marker = root.join("hook-ran");
    let source = source_repo_with_post_checkout_hook(
        &root,
        &format!(
            "#!/bin/sh\nread _line\necho ran > \"{}\"\n",
            marker.display()
        ),
    );

    let result = create_worktree_bounded(&source, &target);

    result.expect("worktree creation should succeed");
    assert!(target.exists(), "worktree directory should be created");
    assert!(
        marker.exists(),
        "post-checkout hook should run to completion (stdin read returns EOF, not a block)"
    );

    let _ = fs::remove_dir_all(root);
}

// The denied-prompt case: with the tty shed, the hook's `sudo`/`ssh` step
// fails and the hook exits non-zero. Git propagates that exit status
// through the checkout, so creation must fail with an ordinary error —
// never hang — and the failed checkout must not strand a half-made
// worktree.
#[cfg(unix)]
#[test]
fn a_failing_hook_fails_creation_and_removes_the_partial_worktree() {
    let root = temp_dir("hook-denied");
    let target = root.join("worktree");
    let marker = root.join("hook-ran");
    let source = source_repo_with_post_checkout_hook(
        &root,
        &format!(
            "#!/bin/sh\nread _line\necho ran > \"{}\"\nexit 1\n",
            marker.display()
        ),
    );

    let result = create_worktree_bounded(&source, &target);

    result.expect_err("a failing post-checkout hook should fail creation");
    assert!(
        marker.exists(),
        "hook should run to its exit (stdin read returns EOF, not a block)"
    );
    assert!(!target.exists(), "partial worktree should be removed");
    let worktrees = run_git(&["worktree", "list", "--porcelain"], &source);
    assert!(
        !worktrees.contains(&target.to_string_lossy().to_string()),
        "partial worktree should be unregistered: {worktrees}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn pr_key_url_stays_available_for_worktree_messages() {
    let key = PrKey {
        repo_slug: "acme/widgets".to_owned(),
        number: 42,
    };

    assert_eq!(key.html_url(), "https://github.com/acme/widgets/pull/42");
}

#[test]
fn git_env_is_scrubbed_for_git_and_gh_commands() {
    // The hermeticity contract: every invocation that reaches git scrubs the
    // inherited git environment so a sandboxed call can't be redirected onto
    // the repo a hook is running inside. This covers direct `git` calls and
    // `gh` (which shells out to git). Assert each variable is marked removed.
    let assert_scrubbed = |label: &str, command: &Command| {
        let envs: Vec<(OsString, Option<OsString>)> = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(|v| v.to_owned())))
            .collect();
        for var in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_COMMON_DIR",
            "GIT_PREFIX",
        ] {
            assert!(
                envs.iter()
                    .any(|(key, value)| key == std::ffi::OsStr::new(var) && value.is_none()),
                "{label} should mark {var} for removal, got {envs:?}"
            );
        }
    };

    assert_scrubbed("git_command", &git_command());

    // gh shells out to git, so this module's gh builder must be scrubbed too —
    // every gh call site (the PR checkout) is built on it.
    assert_scrubbed("gh_command", &gh_command());
}

#[test]
fn checkout_pr_command_is_hardened_and_scoped_to_the_worktree() {
    let command = checkout_pr_command(Path::new("/somewhere/worktree"), 42);

    assert_eq!(command.get_program(), "gh");
    let args: Vec<&std::ffi::OsStr> = command.get_args().collect();
    assert_eq!(args, ["pr", "checkout", "42"]);
    assert_eq!(
        command.get_current_dir(),
        Some(Path::new("/somewhere/worktree"))
    );

    let envs: Vec<(&std::ffi::OsStr, Option<&std::ffi::OsStr>)> = command.get_envs().collect();
    assert!(
        envs.contains(&(
            std::ffi::OsStr::new("GIT_TERMINAL_PROMPT"),
            Some(std::ffi::OsStr::new("0"))
        )),
        "checkout should disable git terminal prompts, got {envs:?}"
    );
    for token in ["GITHUB_TOKEN", "GH_TOKEN"] {
        assert!(
            envs.contains(&(std::ffi::OsStr::new(token), None)),
            "checkout should drop ambient {token}, got {envs:?}"
        );
    }
}
