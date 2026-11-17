//! contents of cli.rs
//! // This module defines the command-line interface (CLI) for the Fusion cloud build tool.

use clap::Args;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct BuildArgs {
    #[arg(short, long, default_value = "./src/main.fusion")]
    pub input: PathBuf,

    #[arg(short, long, default_value = "./target/build")]
    pub out_dir: PathBuf,

    /// Offload compilation to isolated cloud workers
    #[arg(long)]
    pub cloud: bool,

    #[arg(long, default_value = "https://builds.fusion.org")]
    pub cloud_endpoint: String,
}