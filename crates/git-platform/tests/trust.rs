//! The trust store and the token store.

use git_platform::trust::TrustStore;
use tempfile::TempDir;

#[test]
fn trust_is_remembered_per_path_across_loads_and_windows() {
    let tmp = TempDir::new().unwrap();
    let file = tmp.path().join("state/better-os/git-trusted");
    let repo = tmp.path().join("repo with space");
    let other = tmp.path().join("other");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let mut first = TrustStore::load(Some(file.clone()));
    let mut second = TrustStore::load(Some(file.clone()));
    assert!(!first.is_trusted(&repo));
    first.trust(&repo).unwrap();
    second.trust(&other).unwrap();
    let reread = TrustStore::load(Some(file.clone()));
    assert!(
        reread.is_trusted(&repo),
        "the second window kept the first's answer"
    );
    assert!(reread.is_trusted(&other));
    assert!(
        reread.is_trusted(&repo.join(".")),
        "the same folder by another spelling"
    );
    first.forget(&repo).unwrap();
    assert!(!TrustStore::load(Some(file)).is_trusted(&repo));
    let mut memory = TrustStore::load(None);
    memory.trust(&repo).unwrap();
    assert!(memory.is_trusted(&repo));
}
