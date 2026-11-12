//! Pull requests and their reviews.

use serde::Deserialize;
use serde_json::json;

use crate::repos::{Owner, RepoInfo};
use crate::{ApiError, Client, segment};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct PullRef {
    #[serde(rename = "ref")]
    pub name: String,
    pub sha: String,
    #[serde(default)]
    pub repo: Option<RepoInfo>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub html_url: String,
    pub user: Owner,
    pub head: PullRef,
    pub base: PullRef,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub created_at: String,
}

/// A pull request to open. `head` is `branch`, or `owner:branch` for a
/// fork.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NewPull {
    pub title: String,
    pub body: String,
    pub head: String,
    pub base: String,
    pub draft: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Review {
    pub id: u64,
    pub user: Owner,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, ...
    pub state: String,
    #[serde(default)]
    pub html_url: String,
}

impl Client {
    pub fn pull_requests(&self, owner: &str, name: &str) -> Result<Vec<PullRequest>, ApiError> {
        self.get_pages(
            &format!(
                "/repos/{}/{}/pulls?state=open&per_page=100",
                segment(owner),
                segment(name)
            ),
            |page: Vec<PullRequest>| page,
        )
    }

    pub fn create_pull_request(
        &self,
        owner: &str,
        name: &str,
        pull: &NewPull,
    ) -> Result<PullRequest, ApiError> {
        self.post(
            &format!("/repos/{}/{}/pulls", segment(owner), segment(name)),
            &json!({
                "title": pull.title,
                "body": pull.body,
                "head": pull.head,
                "base": pull.base,
                "draft": pull.draft,
            }),
        )
    }

    pub fn reviews(&self, owner: &str, name: &str, number: u64) -> Result<Vec<Review>, ApiError> {
        self.get_pages(
            &format!(
                "/repos/{}/{}/pulls/{number}/reviews?per_page=100",
                segment(owner),
                segment(name)
            ),
            |page: Vec<Review>| page,
        )
    }
}
