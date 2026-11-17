//! Cloud build integration.
//! contents of lib.rs

use std::path::PathBuf;
use std::error::Error;
use serde::{Serialize, Deserialize};

pub mod cli;
pub mod orchestrator;

#[derive(Serialize, Deserialize, Debug)]
pub struct CloudBuildConfig {
    pub target_architecture: String,
    pub language_plugin: String,
    pub source_path: PathBuf,
    pub optimization_level: u8,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BuildResult {
    pub success: bool,
    pub artifact_path: Option<PathBuf>,
    pub logs: String,
    pub execution_time_ms: u64,
}

pub struct CloudBuildPlugin {
    endpoint: String,
    api_token: String,
}

impl CloudBuildPlugin {
    /// Initialize the cloud build client with a remote runner endpoint and auth token.
    pub fn new(endpoint: impl Into<String>, api_token: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_token: api_token.into(),
        }
    }

    /// Submits a remote build request to the cloud orchestrator infrastructure.
    pub async fn execute_remote_build(&self, config: &CloudBuildConfig) -> Result<BuildResult, Box<dyn Error>> {
        let client = reqwest::Client::new();
        
        let response = client
            .post(format!("{}/v1/build", self.endpoint))
            .bearer_auth(&self.api_token)
            .json(config)
            .send()
            .await?;

        if response.status().is_success() {
            let result = response.json::<BuildResult>().await?;
            Ok(result)
        } else {
            Err(format!("Cloud build failed with status code: {}", response.status()).into())
        }
    }
}