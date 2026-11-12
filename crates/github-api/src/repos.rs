//! The account, its repositories and organizations, creating a repository,
//! forking, and the `.gitignore` and license templates.

use serde::Deserialize;
use serde_json::json;

use crate::{ApiError, Client, segment};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct User {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Owner {
    pub login: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Permissions {
    #[serde(default)]
    pub push: bool,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct RepoInfo {
    pub name: String,
    pub full_name: String,
    pub owner: Owner,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub fork: bool,
    #[serde(default)]
    pub description: Option<String>,
    pub clone_url: String,
    pub html_url: String,
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub permissions: Option<Permissions>,
    #[serde(default)]
    pub parent: Option<Box<RepoInfo>>,
}

impl RepoInfo {
    /// Whether this account may push here. Unknown counts as yes; the push
    /// itself will say.
    pub fn can_push(&self) -> bool {
        self.permissions.as_ref().is_none_or(|p| p.push)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Organization {
    pub login: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LicenseSummary {
    pub key: String,
    pub name: String,
}

#[derive(Deserialize)]
struct LicenseBody {
    body: String,
}

#[derive(Deserialize)]
struct GitignoreBody {
    source: String,
}

impl Client {
    pub fn user(&self) -> Result<User, ApiError> {
        self.get("/user")
    }

    /// Every repository the account can reach, most recently updated first.
    pub fn repositories(&self) -> Result<Vec<RepoInfo>, ApiError> {
        self.get_pages(
            "/user/repos?per_page=100&sort=updated&affiliation=owner,collaborator,organization_member",
            |page: Vec<RepoInfo>| page,
        )
    }

    pub fn organizations(&self) -> Result<Vec<Organization>, ApiError> {
        self.get_pages("/user/orgs?per_page=100", |page: Vec<Organization>| page)
    }

    pub fn repository(&self, owner: &str, name: &str) -> Result<RepoInfo, ApiError> {
        self.get(&format!("/repos/{}/{}", segment(owner), segment(name)))
    }

    /// Creates an empty repository for the account, or for `organization`.
    pub fn create_repository(
        &self,
        name: &str,
        description: &str,
        private: bool,
        organization: Option<&str>,
    ) -> Result<RepoInfo, ApiError> {
        let path = match organization {
            Some(org) => format!("/orgs/{}/repos", segment(org)),
            None => "/user/repos".to_string(),
        };
        self.post(
            &path,
            &json!({ "name": name, "description": description, "private": private, "auto_init": false }),
        )
    }

    /// Forks into the account. GitHub answers before the fork is ready; a
    /// push right after may need a retry.
    pub fn fork(&self, owner: &str, name: &str) -> Result<RepoInfo, ApiError> {
        self.post(
            &format!("/repos/{}/{}/forks", segment(owner), segment(name)),
            &json!({}),
        )
    }

    pub fn gitignore_templates(&self) -> Result<Vec<String>, ApiError> {
        self.get("/gitignore/templates")
    }

    pub fn gitignore_template(&self, name: &str) -> Result<String, ApiError> {
        let body: GitignoreBody = self.get(&format!("/gitignore/templates/{}", segment(name)))?;
        Ok(body.source)
    }

    pub fn licenses(&self) -> Result<Vec<LicenseSummary>, ApiError> {
        self.get("/licenses")
    }

    pub fn license(&self, key: &str) -> Result<String, ApiError> {
        let body: LicenseBody = self.get(&format!("/licenses/{}", segment(key)))?;
        Ok(body.body)
    }
}

#[derive(Deserialize)]
struct BranchBody {
    #[serde(default)]
    protected: bool,
}

impl Client {
    /// Whether the branch is protected on GitHub. A branch GitHub does not
    /// have yet is not.
    pub fn branch_protected(
        &self,
        owner: &str,
        name: &str,
        branch: &str,
    ) -> Result<bool, ApiError> {
        match self.get::<BranchBody>(&format!(
            "/repos/{}/{}/branches/{}",
            segment(owner),
            segment(name),
            segment(branch)
        )) {
            Ok(body) => Ok(body.protected),
            Err(ApiError::Status { status: 404, .. }) => Ok(false),
            Err(error) => Err(error),
        }
    }
}
