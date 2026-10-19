//contents of plugin.rs

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginDescriptor {
    pub name: String,
    pub version: String,
    pub kind: PluginKind,

    #[serde(default)]
    pub capabilities: BTreeSet<Capability>,

    #[serde(default)]
    pub requires: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PluginKind {
    Ergonomic,
    Ecosystem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    BuildSource,
    LinkArtifact,
    GenerateBindings,
    ProvideRuntime,
    AnalyzeSource,
    CloudBuild,
    Sqlite,
    Repl,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginError {
    #[error("plugin name and version are required")]
    MissingIdentity,

    #[error("ecosystem plugin '{0}' must declare capabilities")]
    MissingCapability(String),

    #[error("plugin '{plugin}' requires '{requirement}'")]
    MissingRequirement { plugin: String, requirement: String },
}

impl PluginDescriptor {
    pub fn validate(&self, installed_components: &BTreeSet<String>) -> Result<(), PluginError> {
        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            return Err(PluginError::MissingIdentity);
        }

        if self.kind == PluginKind::Ecosystem && self.capabilities.is_empty() {
            return Err(PluginError::MissingCapability(self.name.clone()));
        }

        for requirement in &self.requires {
            if !installed_components.contains(requirement) {
                return Err(PluginError::MissingRequirement {
                    plugin: self.name.clone(),
                    requirement: requirement.clone(),
                });
            }
        }

        Ok(())
    }
}
