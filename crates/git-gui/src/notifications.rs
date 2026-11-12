//! Notifications for the person's pull requests, as GitHub Desktop sends
//! them: a check run failed, or someone reviewed. Each poll's answer is
//! compared with the last one so a notification is sent once.

use std::collections::BTreeSet;

use github_api::CheckState;

use crate::github::MyPull;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Notice {
    ChecksFailed {
        number: u64,
        title: String,
        failed: Vec<String>,
        url: String,
    },
    Reviewed {
        number: u64,
        title: String,
        reviewer: String,
        state: String,
        url: String,
    },
}

/// What has been notified already: failed `(pull, head sha)` pairs and
/// review ids. The first poll fills it without notifying, so opening a
/// repository does not repeat old news.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    primed: bool,
    failures: BTreeSet<(u64, String)>,
    reviews: BTreeSet<u64>,
}

impl Seen {
    pub fn update(&mut self, pulls: &[MyPull]) -> Vec<Notice> {
        let mut out = Vec::new();
        for mine in pulls {
            let pull = &mine.pull;
            if mine.checks.overall() == Some(CheckState::Failure) {
                let key = (pull.number, pull.head.sha.clone());
                if self.failures.insert(key) && self.primed {
                    out.push(Notice::ChecksFailed {
                        number: pull.number,
                        title: pull.title.clone(),
                        failed: mine
                            .checks
                            .failed()
                            .iter()
                            .map(|r| r.name.clone())
                            .collect(),
                        url: pull.html_url.clone(),
                    });
                }
            }
            for review in &mine.reviews {
                if review.state == "PENDING" {
                    continue;
                }
                if self.reviews.insert(review.id) && self.primed {
                    out.push(Notice::Reviewed {
                        number: pull.number,
                        title: pull.title.clone(),
                        reviewer: review.user.login.clone(),
                        state: review.state.clone(),
                        url: if review.html_url.is_empty() {
                            pull.html_url.clone()
                        } else {
                            review.html_url.clone()
                        },
                    });
                }
            }
        }
        self.primed = true;
        out
    }
}

/// Text for a notification: notification servers may read the body as
/// markup (`<b>`, `<a href>`, `<img>`), and a pull request's title is
/// whatever its author typed, so `&`, `<` and `>` are escaped and control
/// characters dropped.
pub fn plain_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if c.is_control() && c != '\n' => {}
            c => out.push(c),
        }
    }
    out
}

/// Shows a desktop notification through `org.freedesktop.Notifications`
/// on the session bus. Failing to reach it is not an error worth showing:
/// the window shows the same news in its own bar.
pub fn send_desktop(summary: &str, body: &str) {
    let summary = plain_text(summary);
    let body = plain_text(body);
    std::thread::spawn(move || {
        let Ok(connection) = zbus::blocking::Connection::session() else {
            return;
        };
        let hints: std::collections::HashMap<&str, zbus::zvariant::Value> =
            std::collections::HashMap::new();
        let _ = connection.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Better Git",
                0u32,
                "better-git",
                summary.as_str(),
                body.as_str(),
                Vec::<&str>::new(),
                hints,
                8000i32,
            ),
        );
    });
}
