//contents of manifest.rs

use serde::Deserialize;
use std::str::FromStr;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectManifest {
    pub package: PackageSection,

    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencySpec>,

    #[serde(rename = "dev-dependencies", default)]
    pub dev_dependencies: BTreeMap<String, DependencySpec>,

    #[serde(default)]
    pub plugins: BTreeMap<String, DependencySpec>,

    #[serde(default)]
    pub toolchain: ToolchainSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageSection {
    pub name: String,

    #[serde(default = "default_version")]
    pub version: String,

    #[serde(default = "default_edition")]
    pub edition: String,

    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolchainSection {
    pub fusion: Option<String>,
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    Version(String),
    Detailed(DetailedDependency),
}

#[derive(Debug, Clone, Deserialize)]
pub struct DetailedDependency {
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub git: Option<String>,
    pub rev: Option<String>,
    pub tag: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid fusion.toml: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("package name must not be empty")]
    EmptyName,

    #[error("invalid dependency '{0}': specify exactly one of version, path, or git")]
    InvalidDependency(String),
}

fn default_version() -> String {
    "0.1.0".into()
}

fn default_edition() -> String {
    "2021".into()
}

impl FromStr for ProjectManifest {
    type Err = ManifestError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let manifest: Self = toml::from_str(input)?;
        manifest.validate()?;
        Ok(manifest)
    }
}

impl ProjectManifest {
    pub fn load(root: &Path) -> Result<Self, ManifestError> {
        let path = root.join("fusion.toml");

        let input = fs::read_to_string(&path).map_err(|source| ManifestError::Read {
            path: path.clone(),
            source,
        })?;

        Self::from_str(&input)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.package.name.trim().is_empty() {
            return Err(ManifestError::EmptyName);
        }

        for (name, dependency) in self
            .dependencies
            .iter()
            .chain(self.dev_dependencies.iter())
            .chain(self.plugins.iter())
        {
            match dependency {
                DependencySpec::Version(version) => {
                    if version.trim().is_empty() {
                        return Err(ManifestError::InvalidDependency(name.clone()));
                    }
                }

                DependencySpec::Detailed(spec) => {
                    let sources = usize::from(spec.version.is_some())
                        + usize::from(spec.path.is_some())
                        + usize::from(spec.git.is_some());

                    if sources != 1 {
                        return Err(ManifestError::InvalidDependency(name.clone()));
                    }

                    if spec.rev.is_some() && spec.git.is_none()
                        || spec.tag.is_some() && spec.git.is_none()
                        || spec.branch.is_some() && spec.git.is_none()
                    {
                        return Err(ManifestError::InvalidDependency(name.clone()));
                    }
                }
            }
        }

        Ok(())
    }
}
