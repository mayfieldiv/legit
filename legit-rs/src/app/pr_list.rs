//! Open PR List Module: PRs for the current Tracked Repo, plus the user's
//! selection cursor, scroll viewport, and fetch phase. Concentrates the
//! invariants that used to be spread across `Model` and `update.rs`.

use crate::github::rest::PR;

#[derive(Clone, Debug, Default)]
pub struct PrList {
    prs: Vec<PR>,
}

impl PrList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, pr: PR) {
        self.prs.push(pr);
    }

    pub fn prs(&self) -> &[PR] {
        &self.prs
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::PrList;
    use crate::github::rest::{PR, PRState};

    fn sample_pr(number: u64) -> PR {
        PR {
            number,
            title: format!("PR #{number}"),
            author: "octocat".to_owned(),
            created_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
            updated_at: chrono::Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0).unwrap(),
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
            head_ref: format!("feature/{number}"),
            base_ref: "main".to_owned(),
            head_repository_owner: "mayfieldiv".to_owned(),
            state: PRState::Open,
        }
    }

    #[test]
    fn pushed_pr_appears_in_the_list() {
        let mut list = PrList::new();

        list.push(sample_pr(42));

        assert_eq!(list.prs().len(), 1);
        assert_eq!(list.prs()[0].number, 42);
    }
}
