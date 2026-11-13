//! Signing in to GitHub and what Better Git asks it: the OAuth device
//! flow, the account's repositories, pull requests, checks, issues,
//! publishing, and forking. Every request is a [`GhRequest`] run on the
//! background thread; its [`GhAnswer`] comes back to the session.
//!
//! The token lives in the desktop keyring (see `git_platform::keyring`).
//! An expiring token is refreshed before a request when it is about to
//! expire, and once more when GitHub answers 401; the new token comes back
//! in the answer so the window stores it.

use git_core::HostedRepository;
use github_api::{
    ApiError, Client, CombinedChecks, DeviceCode, Host, Poll, PullRequest, RepoInfo, Review, Token,
};

/// The signed-in account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Account {
    pub host: Host,
    pub login: String,
    pub token: Token,
}

/// Where sign-in is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum SignIn {
    #[default]
    Idle,
    /// Asking GitHub for a code.
    Starting,
    /// Showing the code; polling every `interval` seconds.
    Waiting {
        code: DeviceCode,
        interval: u64,
    },
    Failed(SignInProblem),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignInProblem {
    Expired,
    Denied,
    Disabled,
    Api(ApiError),
}

/// What to ask GitHub.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GhCall {
    DeviceCode,
    Poll {
        device_code: String,
    },
    User,
    Repositories,
    Organizations,
    /// Pull requests, checks on `branch`, and its protection.
    RepositoryState {
        repo: HostedRepository,
        branch: Option<String>,
    },
    RerunChecks {
        repo: HostedRepository,
        checks: CombinedChecks,
    },
    CreatePull {
        repo: HostedRepository,
        title: String,
        body: String,
        head: String,
        base: String,
        draft: bool,
    },
    CreateIssue {
        repo: HostedRepository,
        title: String,
        body: String,
    },
    Publish {
        name: String,
        description: String,
        private: bool,
        organization: Option<String>,
    },
    Fork {
        repo: HostedRepository,
    },
    /// For notifications: the checks and reviews of this account's open
    /// pull requests.
    MyPulls {
        repo: HostedRepository,
    },
    Templates,
    Template {
        gitignore: Option<String>,
        license: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GhRequest {
    pub host: Host,
    pub token: Option<Token>,
    pub call: GhCall,
}

/// One open pull request of the person's, with its checks and reviews.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MyPull {
    pub pull: PullRequest,
    pub checks: CombinedChecks,
    pub reviews: Vec<Review>,
}

/// Each answer is made once and moved once to the window's thread, so the
/// size of the largest variant costs nothing worth boxing for.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GhResult {
    DeviceCode(DeviceCode),
    Poll(Poll),
    User(github_api::User),
    Repositories(Vec<RepoInfo>),
    Organizations(Vec<String>),
    RepositoryState {
        pulls: Vec<PullRequest>,
        checks: Option<CombinedChecks>,
        protected: bool,
        can_push: bool,
        parent: Option<RepoInfo>,
    },
    Rerun(usize),
    CreatedPull(PullRequest),
    CreatedIssue(github_api::Issue),
    Published(RepoInfo),
    Forked(RepoInfo),
    MyPulls(Vec<MyPull>),
    Templates {
        gitignores: Vec<String>,
        licenses: Vec<(String, String)>,
    },
    Template {
        gitignore: Option<String>,
        license: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GhAnswer {
    pub call: GhCall,
    pub result: Result<GhResult, ApiError>,
    /// A token refreshed on the way, for the window to store.
    pub refreshed: Option<Token>,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl GhRequest {
    /// Runs the call, refreshing the token first when it is about to expire
    /// and once more after a 401.
    pub fn run(self, now: i64) -> GhAnswer {
        let mut refreshed = None;
        let mut token = self.token.clone();
        if let Some(t) = &token
            && t.needs_refresh(now)
            && let Some(refresh) = &t.refresh_token
            && let Ok(new) = Client::new(self.host.clone(), None).refresh_token(refresh, now)
        {
            refreshed = Some(new.clone());
            token = Some(new);
        }
        let client = Client::new(
            self.host.clone(),
            token.as_ref().map(|t| t.access_token.clone()),
        );
        let mut result = call(&client, &self.call, now);
        if result == Err(ApiError::Unauthorized)
            && let Some(refresh) = token.as_ref().and_then(|t| t.refresh_token.clone())
            && let Ok(new) = Client::new(self.host.clone(), None).refresh_token(&refresh, now)
        {
            let client = Client::new(self.host.clone(), Some(new.access_token.clone()));
            refreshed = Some(new);
            result = call(&client, &self.call, now);
        }
        GhAnswer {
            call: self.call,
            result,
            refreshed,
        }
    }
}

fn call(client: &Client, call: &GhCall, now: i64) -> Result<GhResult, ApiError> {
    Ok(match call {
        GhCall::DeviceCode => GhResult::DeviceCode(client.device_code()?),
        GhCall::Poll { device_code } => GhResult::Poll(client.poll_token(device_code, now)?),
        GhCall::User => GhResult::User(client.user()?),
        GhCall::Repositories => GhResult::Repositories(client.repositories()?),
        GhCall::Organizations => GhResult::Organizations(
            client
                .organizations()?
                .into_iter()
                .map(|o| o.login)
                .collect(),
        ),
        GhCall::RepositoryState { repo, branch } => {
            let info = client.repository(&repo.owner, &repo.name)?;
            let pulls = client.pull_requests(&repo.owner, &repo.name)?;
            let (checks, protected) = match branch {
                Some(branch) => (
                    client.checks(&repo.owner, &repo.name, branch).ok(),
                    client
                        .branch_protected(&repo.owner, &repo.name, branch)
                        .unwrap_or(false),
                ),
                None => (None, false),
            };
            GhResult::RepositoryState {
                pulls,
                checks,
                protected,
                can_push: info.can_push(),
                parent: info.parent.map(|p| *p),
            }
        }
        GhCall::RerunChecks { repo, checks } => {
            GhResult::Rerun(client.rerun_failed(&repo.owner, &repo.name, checks)?)
        }
        GhCall::CreatePull {
            repo,
            title,
            body,
            head,
            base,
            draft,
        } => GhResult::CreatedPull(client.create_pull_request(
            &repo.owner,
            &repo.name,
            &github_api::NewPull {
                title: title.clone(),
                body: body.clone(),
                head: head.clone(),
                base: base.clone(),
                draft: *draft,
            },
        )?),
        GhCall::CreateIssue { repo, title, body } => {
            GhResult::CreatedIssue(client.create_issue(&repo.owner, &repo.name, title, body)?)
        }
        GhCall::Publish {
            name,
            description,
            private,
            organization,
        } => GhResult::Published(client.create_repository(
            name,
            description,
            *private,
            organization.as_deref(),
        )?),
        GhCall::Fork { repo } => GhResult::Forked(client.fork(&repo.owner, &repo.name)?),
        GhCall::MyPulls { repo } => {
            let me = client.user()?.login;
            let mut mine = Vec::new();
            for pull in client.pull_requests(&repo.owner, &repo.name)? {
                if pull.user.login != me {
                    continue;
                }
                let checks = client
                    .checks(&repo.owner, &repo.name, &pull.head.sha)
                    .unwrap_or_default();
                let reviews = client
                    .reviews(&repo.owner, &repo.name, pull.number)
                    .unwrap_or_default();
                mine.push(MyPull {
                    pull,
                    checks,
                    reviews,
                });
            }
            GhResult::MyPulls(mine)
        }
        GhCall::Templates => GhResult::Templates {
            gitignores: client.gitignore_templates()?,
            licenses: client
                .licenses()?
                .into_iter()
                .map(|l| (l.key, l.name))
                .collect(),
        },
        GhCall::Template { gitignore, license } => GhResult::Template {
            gitignore: match gitignore {
                Some(name) => Some(client.gitignore_template(name)?),
                None => None,
            },
            license: match license {
                Some(key) => Some(client.license(key)?),
                None => None,
            },
        },
    })
}

/// Whether the repository is on the account's host, so its token applies.
pub fn same_host(account: &Account, repo: &HostedRepository) -> bool {
    account.host.name.eq_ignore_ascii_case(&repo.host)
}

/// The keyring item's secret for an account: the login and the token.
pub fn secret(account: &Account) -> String {
    account.token.to_secret()
}

/// The "Create pull request" page on GitHub for a branch, used when the
/// person wants GitHub's own form.
pub fn compare_url(repo: &HostedRepository, base: &str, head: &str) -> String {
    format!("{}/compare/{}...{}?expand=1", repo.web_url(), base, head)
}

pub fn branch_url(repo: &HostedRepository, branch: &str) -> String {
    format!("{}/tree/{}", repo.web_url(), branch)
}
