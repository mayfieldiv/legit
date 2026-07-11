use chrono::TimeZone;

use crate::{app::model::Model, github::rest::PrKey, secret::Secret};

#[test]
fn debug_redacts_auth_token() {
    let (mut model, _) = Model::new();
    model.auth_token = Some(Secret::new("secret-token".to_owned()));

    let debug = format!("{model:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("secret-token"));
}

fn key(number: u64) -> PrKey {
    PrKey {
        repo_slug: "acme/web".to_owned(),
        number,
    }
}

#[test]
fn fetched_at_is_none_until_stamped() {
    let (model, _) = Model::new();
    assert_eq!(model.fetched_at(&key(1)), None);
}

#[test]
fn stamp_fetched_records_per_pr_and_keeps_the_most_recent_stamp() {
    let (mut model, _) = Model::new();
    let early = chrono::Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap();
    let later = early + chrono::Duration::minutes(5);

    model.stamp_fetched(key(1), early);
    assert_eq!(model.fetched_at(&key(1)), Some(early));
    // Last-write-wins: a re-stamp overwrites with the latest fetch event…
    model.stamp_fetched(key(1), later);
    assert_eq!(model.fetched_at(&key(1)), Some(later));
    // …even when that instant is *earlier* than the prior one (wall-clock is
    // non-monotonic — e.g. an NTP step-back). This pins last-write-wins, not
    // a max(): a max would have kept `later` here.
    model.stamp_fetched(key(1), early);
    assert_eq!(model.fetched_at(&key(1)), Some(early));
    // Stamping #1 never touches another PR's stamp.
    assert_eq!(model.fetched_at(&key(2)), None);
}
