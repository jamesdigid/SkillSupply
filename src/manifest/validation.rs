use std::path::{Path, PathBuf};

use crate::manifest::models::{
    CapabilityManifest, InstallSpec, ReadyProbe, RuntimeSpec, RuntimeType, TestSuitePaths,
};
use crate::manifest::parser::{ManifestParser, YamlManifestParser};
use crate::utils::paths::{MANIFEST_FILENAME, manifest_path_for};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestValidationReport {
    pub path: PathBuf,
    pub valid: bool,
    pub manifest: Option<CapabilityManifest>,
    pub issues: Vec<ValidationIssue>,
}

impl ManifestValidationReport {
    pub fn from_manifest(
        path: PathBuf,
        manifest: CapabilityManifest,
        issues: Vec<ValidationIssue>,
    ) -> Self {
        let valid = issues.is_empty();
        Self {
            path,
            valid,
            manifest: Some(manifest),
            issues,
        }
    }

    pub fn invalid(path: PathBuf, issues: Vec<ValidationIssue>) -> Self {
        Self {
            path,
            valid: false,
            manifest: None,
            issues,
        }
    }
}

pub struct ManifestValidator {
    parser: Box<dyn ManifestParser>,
}

impl Default for ManifestValidator {
    fn default() -> Self {
        Self {
            parser: Box::new(YamlManifestParser),
        }
    }
}

impl ManifestValidator {
    pub fn new(parser: Box<dyn ManifestParser>) -> Self {
        Self { parser }
    }

    pub fn validate_path<P: AsRef<Path>>(&self, path: P) -> ManifestValidationReport {
        let path = path.as_ref();
        let (manifest_path, root_dir) = if path.is_dir() {
            (manifest_path_for(path), path)
        } else {
            let root_dir = path.parent().unwrap_or(path);
            (path.to_path_buf(), root_dir)
        };

        // Distinguishes "this provider has no manifest" from "this manifest is
        // broken"; the raw I/O error reads as a generic file-not-found.
        if !manifest_path.exists() {
            return ManifestValidationReport::invalid(
                manifest_path.clone(),
                vec![ValidationIssue {
                    field: MANIFEST_FILENAME.to_string(),
                    message: format!("not found in {}", root_dir.display()),
                }],
            );
        }

        match self.parser.parse_path(&manifest_path) {
            Ok(manifest) => {
                let issues = validate_manifest(&manifest, root_dir);
                ManifestValidationReport::from_manifest(manifest_path, manifest, issues)
            }
            Err(error) => ManifestValidationReport::invalid(
                manifest_path,
                vec![ValidationIssue {
                    field: MANIFEST_FILENAME.to_string(),
                    message: error.to_string(),
                }],
            ),
        }
    }

    pub fn validate_data(&self, data: &str) -> ManifestValidationReport {
        match self.parser.parse_str(data) {
            Ok(manifest) => {
                let issues = validate_manifest(&manifest, Path::new("."));
                ManifestValidationReport::from_manifest(PathBuf::from("<memory>"), manifest, issues)
            }
            Err(error) => ManifestValidationReport::invalid(
                PathBuf::from("<memory>"),
                vec![ValidationIssue {
                    field: "manifest".to_string(),
                    message: error.to_string(),
                }],
            ),
        }
    }
}

fn validate_manifest(manifest: &CapabilityManifest, root_dir: &Path) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    if manifest.name.trim().is_empty() {
        issues.push(ValidationIssue {
            field: "name".to_string(),
            message: "must not be blank".to_string(),
        });
    }

    if manifest.version.trim().is_empty() {
        issues.push(ValidationIssue {
            field: "version".to_string(),
            message: "must not be blank".to_string(),
        });
    }

    issues.extend(validate_text_list("capabilities", &manifest.capabilities));
    issues.extend(validate_text_list("dependencies", &manifest.dependencies));
    issues.extend(validate_install(manifest.install.as_ref()));
    issues.extend(validate_runtime(&manifest.runtime));
    issues.extend(validate_communication(manifest.communication.as_ref()));
    issues.extend(validate_path_field(
        root_dir,
        "prompt",
        manifest.prompt.as_deref(),
    ));
    issues.extend(validate_tests(root_dir, manifest.tests.as_ref()));
    issues.extend(validate_text_list("keywords", &manifest.keywords));

    issues
}

fn validate_install(install: Option<&InstallSpec>) -> Vec<ValidationIssue> {
    let Some(install) = install else {
        return Vec::new();
    };

    let mut issues = Vec::new();
    issues.extend(validate_text_list("install.requires", &install.requires));
    issues.extend(validate_text_list("install.steps", &install.steps));
    issues
}

fn validate_runtime(runtime: &RuntimeSpec) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    match runtime.kind {
        RuntimeType::Available => {}
        _ => {
            if runtime
                .initialize
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                issues.push(ValidationIssue {
                    field: "runtime.initialize".to_string(),
                    message: "must not be blank".to_string(),
                });
            }
        }
    }

    if let Some(ready) = &runtime.ready {
        match ready {
            ReadyProbe::Process => {}
            ReadyProbe::Command { command } => {
                if command.trim().is_empty() {
                    issues.push(ValidationIssue {
                        field: "runtime.ready.command".to_string(),
                        message: "must not be blank".to_string(),
                    });
                }
            }
            ReadyProbe::Tcp { port } => {
                if *port == 0 {
                    issues.push(ValidationIssue {
                        field: "runtime.ready.port".to_string(),
                        message: "must be greater than zero".to_string(),
                    });
                }
            }
            ReadyProbe::StdioHandshake { expected } => {
                if expected.trim().is_empty() {
                    issues.push(ValidationIssue {
                        field: "runtime.ready.expected".to_string(),
                        message: "must not be blank".to_string(),
                    });
                }
            }
        }
    }

    issues
}

fn validate_communication(
    communication: Option<&crate::manifest::models::CommunicationSpec>,
) -> Vec<ValidationIssue> {
    let Some(communication) = communication else {
        return Vec::new();
    };

    match communication.transport {
        crate::manifest::models::TransportType::Stdio
        | crate::manifest::models::TransportType::Http
        | crate::manifest::models::TransportType::Tcp => {}
    }
    Vec::new()
}

fn validate_tests(root_dir: &Path, tests: Option<&TestSuitePaths>) -> Vec<ValidationIssue> {
    let Some(tests) = tests else {
        return Vec::new();
    };

    let mut issues = Vec::new();
    issues.extend(validate_path_field(
        root_dir,
        "tests.startup",
        tests.startup.as_deref(),
    ));
    issues.extend(validate_path_field(
        root_dir,
        "tests.smoke",
        tests.smoke.as_deref(),
    ));
    issues
}

fn validate_path_field(root_dir: &Path, field: &str, value: Option<&str>) -> Vec<ValidationIssue> {
    let Some(value) = value else {
        return Vec::new();
    };

    if value.trim().is_empty() {
        return vec![ValidationIssue {
            field: field.to_string(),
            message: "must not be blank".to_string(),
        }];
    }

    let path = root_dir.join(value);
    if path.exists() {
        Vec::new()
    } else {
        vec![ValidationIssue {
            field: field.to_string(),
            message: format!("path does not exist: {}", path.display()),
        }]
    }
}

fn validate_text_list(field: &str, values: &[String]) -> Vec<ValidationIssue> {
    values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            if value.trim().is_empty() {
                Some(ValidationIssue {
                    field: format!("{field}[{index}]"),
                    message: "must not be blank".to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}
