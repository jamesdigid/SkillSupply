use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, SkillSupportError};
use crate::manifest::models::{CapabilityManifest, TestSuitePaths};
use crate::runtime::capabilities::RuntimeSession;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationSuite {
    pub name: String,
    #[serde(default)]
    pub steps: Vec<VerificationStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationStep {
    pub operation: String,
    pub expect: VerificationExpectation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationExpectation {
    pub status: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct VerificationRunner;

impl VerificationRunner {
    pub fn verify(
        &self,
        provider_root: &Path,
        manifest: &CapabilityManifest,
        session: &mut RuntimeSession,
    ) -> Result<()> {
        let Some(tests) = &manifest.tests else {
            return Ok(());
        };

        for suite_path in suite_paths(provider_root, tests) {
            let source = fs::read_to_string(&suite_path)?;
            let suite: VerificationSuite = serde_yaml_ng::from_str(&source)?;
            self.verify_suite(&suite, session)?;
        }

        Ok(())
    }

    fn verify_suite(&self, suite: &VerificationSuite, session: &mut RuntimeSession) -> Result<()> {
        for step in &suite.steps {
            let response = session.send_json(&serde_json::json!({
                "operation": step.operation,
            }))?;
            let status = response
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if status != step.expect.status {
                return Err(SkillSupportError::Registry(format!(
                    "verification {} failed: expected status {}, got {}",
                    suite.name, step.expect.status, status
                )));
            }
        }

        Ok(())
    }
}

fn suite_paths(provider_root: &Path, tests: &TestSuitePaths) -> Vec<PathBuf> {
    let mut suites = Vec::new();
    if let Some(startup) = tests.startup.as_deref() {
        suites.push(provider_root.join(startup));
    }
    if let Some(smoke) = tests.smoke.as_deref() {
        suites.push(provider_root.join(smoke));
    }
    suites
}
