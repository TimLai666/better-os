//! A minimal client for the privileged service, used by the container
//! end-to-end check.
//!
//! It exists because `busctl` cannot pass a file descriptor, and because the
//! point of the exercise is to drive the same `DbusPrivilegedExecutor` the CLI
//! and GUI use rather than a hand-rolled substitute. It is an example, so it is
//! never built into a shipped package.
//!
//! Usage:
//!   e2e_client install <release> <arch> <path-to-deb>
//!   e2e_client remove  <release> <arch> <component> <installed-version>
//!
//! Prints the outcome document on success and exits non-zero on refusal.

use std::path::Path;

use manager_ipc::{PROTOCOL_VERSION, WireAction, WireArtifact, WirePlan, WireStep};
use manager_platform::PrivilegedTransactionExecutor;
use manager_platform::privileged::DbusPrivilegedExecutor;
use sha2::{Digest, Sha256};

fn sha256_of(path: &Path) -> Result<(String, u64), String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        total += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        total,
    ))
}

/// A transaction id has to be a lowercase UUID, and this example has no
/// randomness available worth the dependency. The container runs one
/// transaction per invocation, so the id only has to differ between runs.
fn transaction_id(seed: &str) -> String {
    let digest = {
        let mut hasher = Sha256::new();
        hasher.update(seed.as_bytes());
        hasher.update(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos().to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    format!(
        "{}-{}-{}-{}-{}",
        &digest[0..8],
        &digest[8..12],
        &digest[12..16],
        &digest[16..20],
        &digest[20..32]
    )
}

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: e2e_client install <release> <arch> <deb>\n       e2e_client remove <release> <arch> <component> <version>";

    let (action, release, architecture) = match arguments.as_slice() {
        [action, release, architecture, ..] => {
            (action.clone(), release.clone(), architecture.clone())
        }
        _ => return Err(usage.to_string()),
    };

    let executor = DbusPrivilegedExecutor::connect().map_err(|error| error.to_string())?;

    let (step, artifact_path) = match action.as_str() {
        "install" => {
            let path = Path::new(arguments.get(3).ok_or(usage)?);
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("the artifact has no file name")?
                .to_string();
            // The release-asset contract is `{id}_{version}_ubuntu-{release}_{arch}.deb`,
            // so the component and version are readable from the name the
            // packaging step produced.
            let fields: Vec<&str> = filename.split('_').collect();
            let component = fields
                .first()
                .ok_or("unreadable artifact name")?
                .to_string();
            let version = fields.get(1).ok_or("unreadable artifact name")?.to_string();
            let (sha256, size_bytes) = sha256_of(path)?;

            (
                WireStep {
                    component,
                    action: WireAction::Install,
                    before_version: None,
                    after_version: Some(version),
                    artifact: Some(WireArtifact {
                        filename,
                        sha256,
                        size_bytes,
                    }),
                },
                Some(path.to_path_buf()),
            )
        }
        "remove" => (
            WireStep {
                component: arguments.get(3).ok_or(usage)?.clone(),
                action: WireAction::Remove,
                before_version: Some(arguments.get(4).ok_or(usage)?.clone()),
                after_version: None,
                artifact: None,
            },
            None,
        ),
        other => return Err(format!("unknown action {other}\n{usage}")),
    };

    let plan = WirePlan {
        protocol_version: PROTOCOL_VERSION,
        transaction_id: transaction_id(&step.component),
        target_release: release,
        target_architecture: architecture,
        steps: vec![step],
    };

    if let Some(path) = artifact_path {
        let artifact = plan.steps[0]
            .artifact
            .as_ref()
            .expect("an install carries an artifact");
        executor
            .stage_artifact(
                &plan.transaction_id,
                &artifact.filename,
                &artifact.sha256,
                &path,
            )
            .map_err(|error| format!("staging refused: {error}"))?;
    }

    let outcome = executor
        .execute_plan(&plan, &mut |_, _| {})
        .map_err(|error| format!("transaction refused: {error}"))?;

    println!("{}", outcome.to_json().map_err(|error| error.to_string())?);

    // A refused plan comes back as a successful call carrying a failed
    // outcome — the client needs the reports either way — so the exit code has
    // to come from the outcome rather than from the call.
    match outcome.status {
        manager_ipc::OutcomeStatus::Succeeded => Ok(()),
        other => Err(format!("the transaction did not succeed: {other:?}")),
    }
}
