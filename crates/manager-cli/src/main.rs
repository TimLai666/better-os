use better_core::{ComponentCatalog, ComponentId, ComponentManifest};
use clap::{Parser, Subcommand};
use manager_core::{DesiredOperation, InMemoryBackend, InstallationState, Manager};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "better-manager",
    about = "Plan and inspect Better OS components"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    List,
    Validate,
    Status {
        id: Option<String>,
    },
    Plan {
        id: String,
        #[arg(long)]
        update: bool,
    },
}

fn manifest_paths() -> [PathBuf; 3] {
    [
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../components/manifests/better-manager.yaml"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../components/manifests/better-monitor.yaml"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../components/manifests/better-files-example.yaml"),
    ]
}

fn load_catalog() -> Result<ComponentCatalog, Box<dyn std::error::Error>> {
    let manifests = manifest_paths()
        .into_iter()
        .map(|path| {
            Ok(ComponentManifest::parse_yaml(&std::fs::read_to_string(
                path,
            )?)?)
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    Ok(ComponentCatalog::from_manifests(manifests)?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let catalog = load_catalog()?;
    let manager = Manager::new(catalog, InMemoryBackend::default());

    match cli.command {
        Command::List => {
            for manifest in manager.manifests() {
                println!("{} {}", manifest.id, manifest.display_name);
            }
        }
        Command::Validate => println!("valid: {} example manifests", manager.manifests().count()),
        Command::Status { id } => {
            let ids = id
                .into_iter()
                .map(ComponentId::new)
                .collect::<Result<Vec<_>, _>>()?;
            for manifest in manager
                .manifests()
                .filter(|manifest| ids.is_empty() || ids.contains(&manifest.id))
            {
                match manager.status(&manifest.id)? {
                    InstallationState::Installed { version } => {
                        println!("{} installed {}", manifest.id, version)
                    }
                    InstallationState::Available => println!("{} available", manifest.id),
                }
            }
        }
        Command::Plan { id, update } => {
            let id = ComponentId::new(id)?;
            let operation = if update {
                DesiredOperation::Update
            } else {
                DesiredOperation::Install
            };
            let plan = manager.plan(&id, operation)?;
            println!("dry-run: {} step(s)", plan.steps.len());
            for step in plan.steps {
                println!("{}: {:?} — {}", step.component, step.operation, step.detail);
            }
        }
    }
    Ok(())
}
