//! contents of orchestrator.rs

use crate::{CloudBuildConfig, CloudBuildPlugin, BuildResult};
use std::path::Path;
use std::error::Error;
use std::fs;

pub struct OrchestratorBridge {
    plugin: CloudBuildPlugin,
}

impl OrchestratorBridge {
    pub fn new(endpoint: String, token: String) -> Self {
        Self {
            plugin: CloudBuildPlugin::new(endpoint, token),
        }
    }

    /// Prepares workspace assets, invokes remote build, and writes out the resulting binaries.
    pub async fn dispatch_build(
        &self,
        source_dir: &Path,
        target_arch: &str,
        lang: &str,
        opt_level: u8,
        output_dest: &Path,
    ) -> Result<(), Box<dyn Error>> {
        if !source_dir.exists() {
            return Err(format!("Source path does not exist: {:?}", source_dir).into());
        }

        let config = CloudBuildConfig {
            target_architecture: target_arch.to_string(),
            language_plugin: lang.to_string(),
            source_path: source_dir.to_path_buf(),
            optimization_level: opt_level,
        };

        println!("Connecting to Fusion cloud builder for architecture: {}...", target_arch);
        let result: BuildResult = self.plugin.execute_remote_build(&config).await?;

        if result.success {
            println!("Cloud build completed successfully in {}ms", result.execution_time_ms);
            if let Some(artifact) = result.artifact_path {
                if artifact.exists() {
                    fs::copy(&artifact, output_dest)?;
                    println!("Artifact successfully deployed to: {:?}", output_dest);
                }
            }
        } else {
            eprintln!("Cloud build failed:\n{}", result.logs);
            return Err("Remote compilation returned a non-zero exit code.".into());
        }

        Ok(())
    }
}