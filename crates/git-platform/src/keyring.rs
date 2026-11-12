//! Where a GitHub token lives: the desktop keyring, through the Secret
//! Service, and never a file.
//!
//! The window stores the token with GPUI's credentials API, which on Linux
//! is the Secret Service through `oo7`. This module holds what both sides
//! agree on, the item's name and what is stored, and a [`TokenStore`]
//! trait so the session can be tested with [`MemoryTokenStore`] and never
//! touch the person's own keyring.

use std::collections::HashMap;
use std::sync::Mutex;

/// The account a token belongs to.
#[derive(Clone, Eq, PartialEq)]
pub struct Account {
    /// `github.com`, or an Enterprise host.
    pub host: String,
    pub login: String,
    pub token: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("host", &self.host)
            .field("login", &self.login)
            .field("token", &"<hidden>")
            .finish()
    }
}

/// The keyring item's URL attribute for one host. Better Git's own prefix
/// keeps it apart from any other program's item for the same host.
pub fn credential_url(host: &str) -> String {
    format!("better-git://{}", host.to_ascii_lowercase())
}

/// Somewhere a token can be kept.
pub trait TokenStore: Send + Sync {
    fn load(&self, host: &str) -> Option<Account>;
    fn save(&self, account: &Account) -> Result<(), String>;
    fn delete(&self, host: &str) -> Result<(), String>;
}

/// A store that forgets everything when dropped, for tests.
#[derive(Default)]
pub struct MemoryTokenStore {
    accounts: Mutex<HashMap<String, Account>>,
}

impl TokenStore for MemoryTokenStore {
    fn load(&self, host: &str) -> Option<Account> {
        self.accounts.lock().ok()?.get(host).cloned()
    }

    fn save(&self, account: &Account) -> Result<(), String> {
        self.accounts
            .lock()
            .map_err(|e| e.to_string())?
            .insert(account.host.clone(), account.clone());
        Ok(())
    }

    fn delete(&self, host: &str) -> Result<(), String> {
        self.accounts
            .lock()
            .map_err(|e| e.to_string())?
            .remove(host);
        Ok(())
    }
}
