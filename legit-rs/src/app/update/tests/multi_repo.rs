// ── multi-repo fan-out ────────────────────────────────────────────────────

use super::*;

#[test]
fn fetch_fans_out_to_every_tracked_repo() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });

    let cmds = update(
        &mut model,
        Msg::ConfigLoaded(config_with_repos(&["acme/web", "acme/api"])),
    );

    // Config repos in config order, then the CWD-detected repo appended.
    assert_eq!(
        fetched_slugs(&cmds),
        ["acme/web", "acme/api", "mayfieldiv/legit"]
    );
    assert!(model.list.is_loading(Some("acme/web")));
    assert!(model.list.is_loading(Some("acme/api")));
    assert!(model.list.is_loading(Some("mayfieldiv/legit")));
}

#[test]
fn detected_repo_already_in_config_is_fetched_once_with_config_casing() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });

    // GitHub slugs are case-insensitive; the configured casing wins.
    let cmds = update(
        &mut model,
        Msg::ConfigLoaded(config_with_repos(&["MayfieldIV/Legit"])),
    );

    assert_eq!(fetched_slugs(&cmds), ["MayfieldIV/Legit"]);
}

#[test]
fn pr_list_loaded_fans_out_enrichment_only_for_that_repo() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("ghp_test".to_owned()));
    // `acme/web` is a tracked repo so its slug resolves back to a `RepoInfo`;
    // `mayfieldiv/legit` is the CWD-detected repo.
    model.config = config_with_repos(&["acme/web"]);
    model.repo = RepoDetection::Detected(RepoInfo {
        owner: "mayfieldiv".to_owned(),
        repo: "legit".to_owned(),
    });
    model.list.begin_fetch("acme/web");
    model.list.begin_fetch("mayfieldiv/legit");
    model.list.push(sample_pr_in("acme/web", 7, "other repo"));
    model.list.push(sample_pr(1, "this repo"));

    let cmds = update(
        &mut model,
        Msg::PrListLoaded {
            repo_slug: "acme/web".to_owned(),
        },
    );

    // One batched review-status + threads/reviews/comments for acme/web#7
    // only; mayfieldiv/legit#1 waits for its own repo's listing to settle.
    assert_eq!(cmds.len(), 1 + 3);
    match &cmds[0] {
        Cmd::FetchReviewStatus { ctx, pr_numbers } => {
            assert_eq!(ctx.repo.slug(), "acme/web");
            assert_eq!(pr_numbers, &[7]);
        }
        other => panic!("first cmd should batch review status, got {other:?}"),
    }
}

#[test]
fn same_pr_number_in_two_repos_does_not_collide() {
    let mut model = enriched_model(&[]);
    model.list.push(sample_pr_in("acme/web", 7, "a"));
    model.list.push(sample_pr(7, "b"));
    let acme_key = PrKey {
        repo_slug: "acme/web".to_owned(),
        number: 7,
    };

    // Full enrichment for acme/web#7 only.
    update(
        &mut model,
        Msg::ThreadsArrived {
            pr: acme_key.clone(),
            threads: Vec::new(),
        },
    );
    update(
        &mut model,
        Msg::ReviewsArrived {
            pr: acme_key.clone(),
            reviews: Vec::new(),
        },
    );

    assert!(
        model.blockers.contains_key(&acme_key),
        "acme/web#7 is classified"
    );
    assert!(
        !model.blockers.contains_key(&key(7)),
        "mayfieldiv/legit#7 must still be loading — its enrichment never arrived"
    );
}
