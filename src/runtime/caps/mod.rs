use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use crate::error::{Result, SkillSupportError};
use crate::manifest::models::{CapabilityManifest, ReadyProbe, RuntimeType};

pub struct RuntimeSession {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

impl RuntimeSession {
    pub fn available() -> Self {
        Self {
            child: None,
            stdin: None,
            stdout: None,
        }
    }

    pub fn send_json(&mut self, value: &serde_json::Value) -> Result<serde_json::Value> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(SkillSupportError::Registry(
                "runtime session does not support stdin communication".to_string(),
            ));
        };
        let Some(stdout) = self.stdout.as_mut() else {
            return Err(SkillSupportError::Registry(
                "runtime session does not support stdout communication".to_string(),
            ));
        };

        writeln!(stdin, "{}", serde_json::to_string(value)?)?;
        stdin.flush()?;

        let mut response = String::new();
        stdout.read_line(&mut response)?;
        if response.trim().is_empty() {
            return Err(SkillSupportError::Registry(
                "provider returned an empty response".to_string(),
            ));
        }

        Ok(serde_json::from_str(response.trim())?)
    }

    pub fn shutdown(mut self, manifest: &CapabilityManifest, root: &Path) -> Result<()> {
        if let Some(command) = manifest.runtime.shutdown.as_deref() {
            let status = Command::new("sh")
                .arg("-lc")
                .arg(command)
                .current_dir(root)
                .status()?;
            if !status.success() {
                return Err(SkillSupportError::Registry(format!(
                    "shutdown command failed for {}",
                    manifest.name
                )));
            }
        }

        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RuntimeController;

impl RuntimeController {
    pub fn start(&self, manifest: &CapabilityManifest, root: &Path) -> Result<RuntimeSession> {
        if matches!(manifest.runtime.kind, RuntimeType::Available) {
            return Ok(RuntimeSession::available());
        }

        let command = manifest.runtime.initialize.as_deref().ok_or_else(|| {
            SkillSupportError::Registry(format!(
                "missing runtime initialize command for {}",
                manifest.name
            ))
        })?;

        let mut child = Command::new("sh")
            .arg("-lc")
            .arg(command)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);
        let mut session = RuntimeSession {
            child: Some(child),
            stdin,
            stdout,
        };

        self.wait_ready(manifest, root, &mut session)?;
        Ok(session)
    }

    fn wait_ready(
        &self,
        manifest: &CapabilityManifest,
        root: &Path,
        session: &mut RuntimeSession,
    ) -> Result<()> {
        let Some(ready) = &manifest.runtime.ready else {
            return Ok(());
        };

        match ready {
            ReadyProbe::Process => Ok(()),
            ReadyProbe::Command { command } => {
                let status = Command::new("sh")
                    .arg("-lc")
                    .arg(command)
                    .current_dir(root)
                    .status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(SkillSupportError::Registry(format!(
                        "ready command failed for {}",
                        manifest.name
                    )))
                }
            }
            ReadyProbe::Tcp { port } => TcpStream::connect(("127.0.0.1", *port))
                .map(|_| ())
                .map_err(SkillSupportError::from),
            ReadyProbe::StdioHandshake { expected } => {
                let Some(stdout) = session.stdout.as_mut() else {
                    return Err(SkillSupportError::Registry(
                        "stdio handshake requires captured stdout".to_string(),
                    ));
                };

                let mut line = String::new();
                stdout.read_line(&mut line)?;
                if line.trim() == expected.trim() {
                    Ok(())
                } else {
                    Err(SkillSupportError::Registry(format!(
                        "ready handshake mismatch for {}: expected {expected:?}, got {line:?}",
                        manifest.name
                    )))
                }
            }
        }
    }
}
