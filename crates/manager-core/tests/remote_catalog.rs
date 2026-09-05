//! The end-to-end proof: a manager holding only its compiled-in catalog
//! refreshes from the published one and can then plan an install whose
//! checksum is verified against the real artifact.
//!
//! It is `#[ignore]`d because it needs the public network, and the default test
//! suite must not. Run it deliberately:
//!
//! ```text
//! cargo test -p manager-core --test remote_catalog -- --ignored --nocapture
//! ```
//!
//! Nothing here touches the host. It fetches manifests, plans, and downloads
//! one `.deb` into a temporary directory to hash it. No package is installed.

use std::fs;

use better_core::ComponentId;
use manager_core::catalog::{
    CatalogSource, CatalogStatus, built_in_catalog, catalog_files, now_unix_seconds, refresh,
};
use manager_core::{DesiredOperation, Manager, ManagerState};
use manager_platform::catalog_fetch::{HttpManifestFetcher, ManifestFetcher};
use manager_platform::download::{ArtifactCache, HttpDownloader};
use manager_platform::{DownloadRequest, SystemProfile};

#[test]
#[ignore = "reaches the public network"]
fn a_manager_refreshes_the_published_catalog_and_plans_a_verified_install() {
    let built_in = built_in_catalog();
    let status = CatalogStatus::built_in();
    println!(
        "built-in catalog: {} components, state {:?}",
        built_in.manifests().count(),
        status.degraded
    );

    let fetcher = HttpManifestFetcher::from_environment();
    println!("fetching from {}", fetcher.source_url());
    let outcome = refresh(&fetcher, &built_in, &status, now_unix_seconds());

    for rejection in &outcome.status.rejections {
        println!("rejected {rejection}");
    }
    assert_eq!(
        outcome.accepted,
        catalog_files().len(),
        "every published manifest must be fetched and validated"
    );
    assert_eq!(outcome.status.source, CatalogSource::Remote);
    assert_eq!(outcome.status.degraded, None);
    let cache = outcome.cache.expect("a landed refresh produces a cache");
    println!(
        "refreshed {} manifests from {} at {}",
        outcome.accepted, cache.source_url, cache.fetched_at_unix_seconds
    );

    // Plan a real component's install against a target the release actually
    // builds for, from the refreshed catalog alone.
    let manager = Manager::new(
        outcome.catalog,
        SystemProfile {
            distribution: "ubuntu".to_string(),
            release: "24.04".to_string(),
            architecture: "amd64".to_string(),
            free_disk_bytes: Some(8 * 1024 * 1024 * 1024),
        },
    );
    let component = ComponentId::new("better-monitor").unwrap();
    let plan = manager
        .plan(
            &ManagerState::default(),
            &component,
            DesiredOperation::Install,
        )
        .expect("the refreshed catalog plans an install");
    let step = plan
        .steps()
        .iter()
        .find(|step| step.component == component)
        .expect("the plan names the component");
    let artifact = step
        .artifact
        .clone()
        .expect("an install step carries the artifact it would fetch");
    let url = artifact.url.clone().expect("the manifest declares a URL");
    println!(
        "planned {} -> {} from {}",
        step.component,
        step.after_version.as_deref().unwrap_or("unknown"),
        url
    );

    // The checksum is only worth something if the published bytes actually
    // hash to it, so the artifact is fetched and verified rather than trusted.
    let root = std::env::temp_dir().join(format!(
        "better-os-remote-catalog-proof-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let downloader = HttpDownloader::new(ArtifactCache::new(&root));
    let receipt = downloader
        .fetch(
            &DownloadRequest {
                component: component.clone(),
                url,
                sha256: artifact.sha256.clone(),
                expected_bytes: artifact.expected_bytes,
            },
            &mut |_| {},
        )
        .expect("the published artifact downloads and matches its declared checksum");
    assert_eq!(receipt.verified_sha256, artifact.sha256);
    println!(
        "verified {} bytes of {} against {}",
        receipt.bytes, artifact.release_asset, receipt.verified_sha256
    );
    let _ = fs::remove_dir_all(&root);
}
