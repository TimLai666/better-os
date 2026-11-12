//! The token store's contract, with the in-memory store tests use. The real
//! store is the desktop keyring; no test touches it.

use git_platform::keyring::{Account, MemoryTokenStore, TokenStore, credential_url};

#[test]
fn memory_store_saves_loads_and_deletes_and_hides_the_token() {
    let store = MemoryTokenStore::default();
    assert!(store.load("github.com").is_none());
    let account = Account {
        host: "github.com".into(),
        login: "octo".into(),
        token: "secret".into(),
    };
    store.save(&account).unwrap();
    assert_eq!(store.load("github.com"), Some(account.clone()));
    assert!(!format!("{account:?}").contains("secret"));
    store.delete("github.com").unwrap();
    assert!(store.load("github.com").is_none());
    assert_eq!(credential_url("GitHub.com"), "better-git://github.com");
}
