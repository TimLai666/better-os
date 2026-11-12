//! Creating an issue.

use serde::Deserialize;
use serde_json::json;

use crate::{ApiError, Client, segment};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Issue {
    pub number: u64,
    pub html_url: String,
}

impl Client {
    pub fn create_issue(
        &self,
        owner: &str,
        name: &str,
        title: &str,
        body: &str,
    ) -> Result<Issue, ApiError> {
        self.post(
            &format!("/repos/{}/{}/issues", segment(owner), segment(name)),
            &json!({ "title": title, "body": body }),
        )
    }
}
