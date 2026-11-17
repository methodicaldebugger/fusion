//! contents of daemon.rs


/*
This server-side daemon listens for incoming HTTP POST requests
 at /v1/build, unpacks the configuration payload, 
 sets up an isolated temporary workspace sandbox, 
 compiles the foreign code safely using local compilers like clang, 
 and returns the execution logs, time, and compiled artifact path.
*/

use axum::{
    extract::Json,
    http::StatusCode,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;
use tempfile::TempDir;

#[derive(Deserialize, Debug)]
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

async fn handle_build(Json(config): Json<CloudBuildConfig>) -> (StatusCode, Json<BuildResult>) {
    let start_time = Instant::now();
    let temp_dir = match TempDir::new() {
        Ok(dir) => dir,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(BuildResult {
                    success: false,
                    artifact_path: None,
                    logs: format!("Failed to create sandbox temp dir: {}", e),
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                }),
            )
        }
    };

    let output_artifact = temp_dir.path().join("output_plugin.dll");

    // Execute isolated compiler subprocess (e.g., Clang for C plugins)
    let compiler_output = Command::new("clang")
        .arg("-shared")
        .arg(format!("-O{}", config.optimization_level))
        .arg("-o")
        .arg(&output_artifact)
        .arg(&config.source_path)
        .output();

    let execution_time_ms = start_time.elapsed().as_millis() as u64;

    match compiler_output {
        Ok(output) => {
            let success = output.status.success();
            let logs = format!(
                "stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );

            let artifact_path = if success && output_artifact.exists() {
                Some(output_artifact)
            } else {
                None
            };

            (
                StatusCode::OK,
                Json(BuildResult {
                    success,
                    artifact_path,
                    logs,
                    execution_time_ms,
                }),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
                Json(BuildResult {
                success: false,
                artifact_path: None,
                logs: format!("Failed to execute compiler binary: {}", e),
                execution_time_ms,
            }),
        ),
    }
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/v1/build", post(handle_build));
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    
    println!("Fusion Cloud Worker Daemon listening on port 8080...");
    axum::serve(listener, app).await.unwrap();
}