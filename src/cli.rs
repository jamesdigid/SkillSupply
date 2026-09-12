use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::{Parser, Subcommand};

use crate::error::SkillSupportError;
use crate::learn::{LearnEngine, LearnOptions};
use crate::manifest::validation::ManifestValidator;
use crate::manifest::{CapabilityManifest, WorkspaceManifest};
use crate::resolver::{DependencyResolver, PetgraphDependencyResolver};
use crate::runtime::config::RuntimeConfig;
use crate::storage::{CapabilitySource, FilesystemCapabilityLoader};
use crate::utils::paths::{WORKSPACE_DOC_FILENAME, WORKSPACE_FILENAME, caps_dir_for};

#[derive(Debug, Clone)]
struct ValidationRow {
    field: String,
    value: String,
    status: String,
}

#[derive(Debug, Parser)]
#[command(
    name = "caps",
    about = "Open capability registry and capability manager for autonomous agents",
    version,
    propagate_version = true
)]
pub struct Cli {
    #[arg(long, global = true)]
    registry: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Scaffold a new workspace directory.
    Init {
        /// Directory to initialize. Defaults to the parent of the current working directory.
        directory: Option<PathBuf>,
    },
    Validate {
        path: PathBuf,
    },
    Install,
    /// Learn a capability into the current workspace.
    Learn {
        capability: String,
    },
    Graph,
    Search,
    /// Start the local SkillSupport developer runtime.
    Dev {
        /// Path to a TOML config file with a [runtime] section.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Override the runtime transport host.
        #[arg(long)]
        host: Option<String>,
        /// Override the runtime transport port.
        #[arg(long)]
        port: Option<u16>,
    },
}

pub fn run() -> crate::error::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { directory } => {
            init(directory)?;
        }
        Commands::Validate { path } => {
            validate(path);
        }
        Commands::Install => {
            println!("install is not implemented yet");
        }
        Commands::Learn { capability } => {
            let engine = LearnEngine;
            let outcome = engine.learn(
                &capability,
                &LearnOptions {
                    registry_dir: cli.registry,
                },
            )?;
            println!(
                "learned capability {} into {}",
                outcome.capability,
                outcome.provider_root.display()
            );
        }
        Commands::Graph => {
            print_installed_capability_graph()?;
        }
        Commands::Search => {
            println!("search is not implemented yet");
        }
        Commands::Dev { config, host, port } => {
            dev(config, host, port)?;
        }
    }

    Ok(())
}

fn dev(
    config_path: Option<PathBuf>,
    host: Option<String>,
    port: Option<u16>,
) -> crate::error::Result<()> {
    let config = match config_path {
        Some(path) => RuntimeConfig::load(&path)?,
        None => RuntimeConfig::default(),
    }
    .with_host(host)
    .with_port(port);

    println!("SkillSupport Runtime\n");
    println!("Transport: {}\n", config.transport_url());
    println!("Waiting for capabilities...");

    let shutdown = Arc::new(AtomicBool::new(false));
    let signal_shutdown = Arc::clone(&shutdown);
    ctrlc::set_handler(move || {
        signal_shutdown.store(true, Ordering::SeqCst);
    })
    .map_err(|error| SkillSupportError::Transport(error.to_string()))?;

    crate::runtime::serve(&config, shutdown)
}

fn init(target_dir: Option<PathBuf>) -> crate::error::Result<()> {
    let cwd = env::current_dir()?;
    let target_dir = match target_dir {
        Some(directory) => directory,
        None => cwd.clone(),
    };

    let caps_dir = caps_dir_for(&target_dir);
    fs::create_dir_all(&caps_dir)?;

    let workspace_name = target_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("skillsupport-workspace");
    let manifest_path = target_dir.join(WORKSPACE_FILENAME);
    let workspace_doc_path = target_dir.join(WORKSPACE_DOC_FILENAME);

    let workspace = WorkspaceManifest {
        name: workspace_name.to_string(),
        version: "0.1.0".to_string(),
        capabilities: Vec::new(),
    };

    let workspace_doc = concat!(
        "# Caps Workspace\n\n",
        "This directory was initialized by `caps init`.\n\n",
        "Installed capabilities live under `caps/`.\n\n",
        "## Next Steps\n\n",
        "- Add installed capabilities under `caps/`.\n",
        "- Use `caps learn <capability>` to acquire a capability.\n"
    )
    .to_string();

    write_if_missing(&manifest_path, &serde_yaml_ng::to_string(&workspace)?)?;
    write_if_missing(&workspace_doc_path, &workspace_doc)?;

    println!(
        "initialized capability workspace in {}",
        target_dir.display()
    );
    Ok(())
}

fn write_if_missing(path: &Path, contents: &str) -> crate::error::Result<()> {
    if path.exists() {
        return Ok(());
    }

    fs::write(path, contents)?;
    Ok(())
}

fn validate(path: PathBuf) {
    let validator = ManifestValidator::default();
    let report = validator.validate_path(&path);
    let rows = validation_rows(&report);
    print_validation_rows(&rows);
}

fn validation_rows(report: &crate::manifest::ManifestValidationReport) -> Vec<ValidationRow> {
    let mut rows = rows_from_fields([("path", report.path.display().to_string())], report.valid);

    if let Some(manifest) = report.manifest.as_ref() {
        rows.extend(manifest_rows(manifest, report.valid));
    }

    rows.extend(issue_rows(&report.issues));
    rows
}

fn manifest_rows(manifest: &CapabilityManifest, valid: bool) -> Vec<ValidationRow> {
    let mut fields = vec![
        ("name", manifest.name.clone()),
        ("version", manifest.version.clone()),
        ("capabilities", joined(&manifest.capabilities)),
        ("dependencies", joined(&manifest.dependencies)),
        ("keywords", joined(&manifest.keywords)),
        ("runtime", format!("{:?}", manifest.runtime.kind)),
    ];

    if let Some(install) = manifest.install.as_ref() {
        fields.extend([
            ("install.requires", joined(&install.requires)),
            ("install.steps", joined(&install.steps)),
        ]);
    }

    if let Some(prompt) = manifest.prompt.as_ref() {
        fields.push(("prompt", prompt.clone()));
    }

    if let Some(tests) = manifest.tests.as_ref() {
        fields.extend([
            (
                "tests.startup",
                tests.startup.clone().unwrap_or_else(|| "-".to_string()),
            ),
            (
                "tests.smoke",
                tests.smoke.clone().unwrap_or_else(|| "-".to_string()),
            ),
        ]);
    }

    rows_from_fields(fields, valid)
}

fn issue_rows(issues: &[crate::manifest::validation::ValidationIssue]) -> Vec<ValidationRow> {
    issues
        .iter()
        .map(|issue| {
            validation_row(
                format!("issue: {}", issue.field),
                issue.message.clone(),
                false,
            )
        })
        .collect()
}

fn validation_row(
    field: impl Into<String>,
    value: impl Into<String>,
    valid: bool,
) -> ValidationRow {
    ValidationRow {
        field: field.into(),
        value: value.into(),
        status: status(valid).to_string(),
    }
}

fn rows_from_fields(
    fields: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    valid: bool,
) -> Vec<ValidationRow> {
    fields
        .into_iter()
        .map(|(field, value)| validation_row(field, value, valid))
        .collect()
}

fn print_installed_capability_graph() -> crate::error::Result<()> {
    let cwd = env::current_dir()?;
    let caps_dir = caps_dir_for(&cwd);
    if !caps_dir.exists() {
        println!("no installed capabilities found");
        return Ok(());
    }

    let loader = FilesystemCapabilityLoader::default();
    let mut manifests = Vec::new();
    for entry in fs::read_dir(&caps_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        // Skips the provider staging area and any other bookkeeping directory.
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if let Ok(capability) = loader.load(&entry.path()) {
            manifests.push(capability.manifest().clone());
        }
    }

    if manifests.is_empty() {
        println!("no installed capabilities found");
        return Ok(());
    }

    let resolver = PetgraphDependencyResolver;
    let graph = resolver.build(&manifests)?;
    println!("nodes: {}", graph.nodes().join(", "));
    println!("edges: {}", graph.edges().len());
    println!("order: {}", graph.topological_order()?.join(", "));
    let unresolved = graph.unresolved_requirements();
    if !unresolved.is_empty() {
        println!("unresolved: {}", unresolved.join(", "));
    }
    Ok(())
}

fn joined(values: &[String]) -> String {
    if values.is_empty() {
        "-".to_string()
    } else {
        values.join(", ")
    }
}

fn status(valid: bool) -> &'static str {
    if valid { "valid" } else { "invalid" }
}

fn print_validation_rows(rows: &[ValidationRow]) {
    let field_width = rows
        .iter()
        .map(|row| row.field.len())
        .chain(["Field".len()])
        .max()
        .unwrap_or(5);
    let value_width = rows
        .iter()
        .map(|row| row.value.len())
        .chain(["Value".len()])
        .max()
        .unwrap_or(5);
    let status_width = rows
        .iter()
        .map(|row| row.status.len())
        .chain(["Status".len()])
        .max()
        .unwrap_or(6);

    let separator = format!(
        "+-{:-<field_width$}-+-{:-<value_width$}-+-{:-<status_width$}-+",
        "",
        "",
        "",
        field_width = field_width,
        value_width = value_width,
        status_width = status_width,
    );

    println!("{separator}");
    println!(
        "| {:<field_width$} | {:<value_width$} | {:<status_width$} |",
        "Field",
        "Value",
        "Status",
        field_width = field_width,
        value_width = value_width,
        status_width = status_width,
    );
    println!("{separator}");
    for row in rows {
        println!(
            "| {:<field_width$} | {:<value_width$} | {:<status_width$} |",
            row.field,
            row.value,
            row.status,
            field_width = field_width,
            value_width = value_width,
            status_width = status_width,
        );
    }
    println!("{separator}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    struct CwdGuard {
        previous: PathBuf,
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.previous);
        }
    }

    #[test]
    fn init_scaffolds_an_explicit_directory() {
        let tempdir = tempdir().expect("temp dir");

        init(Some(tempdir.path().to_path_buf())).expect("init should succeed");

        assert_scaffold(tempdir.path());
    }

    #[test]
    fn init_defaults_to_the_current_directory() {
        let tempdir = tempdir().expect("temp dir");
        let nested_dir = tempdir.path().join("agent");
        fs::create_dir_all(&nested_dir).expect("nested dir");

        let previous = env::current_dir().expect("current dir");
        let _guard = CwdGuard { previous };
        env::set_current_dir(&nested_dir).expect("switch cwd");

        init(None).expect("init should succeed");

        assert_scaffold(&nested_dir);
    }

    fn assert_scaffold(target_dir: &Path) {
        let manifest_path = target_dir.join(WORKSPACE_FILENAME);
        let workspace_doc_path = target_dir.join(WORKSPACE_DOC_FILENAME);
        let caps_dir = caps_dir_for(target_dir);

        assert!(manifest_path.exists());
        assert!(workspace_doc_path.exists());
        assert!(caps_dir.exists());
        assert!(fs::read_dir(&caps_dir).expect("caps dir").next().is_none());

        let manifest = fs::read_to_string(manifest_path).expect("manifest");
        assert!(manifest.contains("version: 0.1.0"));

        let workspace_doc = fs::read_to_string(workspace_doc_path).expect("workspace doc");
        assert!(workspace_doc.contains("Installed capabilities live under `caps/`"));
    }
}
