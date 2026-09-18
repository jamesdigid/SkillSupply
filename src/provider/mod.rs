use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use crate::error::{Result, SkillSupportError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderScheme {
    Github,
    Local,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRef {
    pub scheme: ProviderScheme,
    pub location: String,
    pub reference: Option<String>,
}

impl ProviderRef {
    pub fn slug(&self) -> String {
        self.location
            .rsplit('/')
            .next()
            .unwrap_or(&self.location)
            .replace(['@', ':'], "-")
    }

    pub fn git_url(&self) -> Option<String> {
        match self.scheme {
            ProviderScheme::Github => Some(format!(
                "{}/{}.git",
                github_base_url().trim_end_matches('/'),
                self.location
            )),
            ProviderScheme::Local | ProviderScheme::File => None,
        }
    }

    pub fn as_path(&self) -> PathBuf {
        PathBuf::from(&self.location)
    }
}

impl Display for ProviderRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.scheme {
            ProviderScheme::Github => {
                if let Some(reference) = &self.reference {
                    write!(f, "github:{}@{}", self.location, reference)
                } else {
                    write!(f, "github:{}", self.location)
                }
            }
            ProviderScheme::Local => write!(f, "local:{}", self.location),
            ProviderScheme::File => write!(f, "file:{}", self.location),
        }
    }
}

impl FromStr for ProviderRef {
    type Err = SkillSupportError;

    fn from_str(value: &str) -> Result<Self> {
        let (scheme, remainder) = value.split_once(':').ok_or_else(|| {
            SkillSupportError::Registry(format!("invalid provider reference: {value}"))
        })?;

        let (location, reference) = match remainder.rsplit_once('@') {
            Some((location, reference)) if scheme == "github" => {
                (location.to_string(), Some(reference.to_string()))
            }
            _ => (remainder.to_string(), None),
        };

        let scheme = match scheme {
            "github" => ProviderScheme::Github,
            "local" => ProviderScheme::Local,
            "file" => ProviderScheme::File,
            other => {
                return Err(SkillSupportError::Registry(format!(
                    "unsupported provider scheme: {other}"
                )));
            }
        };

        Ok(Self {
            scheme,
            location,
            reference,
        })
    }
}

pub trait ProviderFetcher {
    fn fetch(&self, provider: &ProviderRef, destination: &Path) -> Result<PathBuf>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalProviderFetcher;

impl ProviderFetcher for LocalProviderFetcher {
    fn fetch(&self, provider: &ProviderRef, destination: &Path) -> Result<PathBuf> {
        let source = provider.as_path();
        let source = if source.is_absolute() {
            source
        } else {
            std::env::current_dir()?.join(source)
        };
        let destination = destination.join(provider.slug());
        if destination.exists() {
            fs::remove_dir_all(&destination)?;
        }
        copy_dir_all(&source, &destination)?;
        Ok(destination)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct GitProviderFetcher;

impl ProviderFetcher for GitProviderFetcher {
    fn fetch(&self, provider: &ProviderRef, destination: &Path) -> Result<PathBuf> {
        let url = provider.git_url().ok_or_else(|| {
            SkillSupportError::Registry("provider is not a github reference".to_string())
        })?;
        let destination = destination.join(provider.slug());
        if destination.exists() {
            fs::remove_dir_all(&destination)?;
        }

        let status = Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg(&url)
            .arg(&destination)
            .status()?;
        if !status.success() {
            return Err(SkillSupportError::Registry(format!(
                "git clone failed for {provider}"
            )));
        }

        if let Some(reference) = &provider.reference {
            let checkout = Command::new("git")
                .arg("-C")
                .arg(&destination)
                .arg("checkout")
                .arg(reference)
                .status()?;
            if !checkout.success() {
                return Err(SkillSupportError::Registry(format!(
                    "git checkout failed for {provider}"
                )));
            }
        }

        Ok(destination)
    }
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn github_base_url() -> String {
    std::env::var("SKILLSUPPORT_GITHUB_BASE_URL")
        .unwrap_or_else(|_| "https://github.com".to_string())
}
