//! contents of cli.rs

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "cloud-build", about = "Offload Fusion foreign builds to isolated cloud workers")]
pub struct CloudBuildCli {
    #[arg(short, long, default_value = "https://builds.fusion.org")]
    pub endpoint: String,

    #[arg(short, long, env("FUSION_BUILD_TOKEN"))]
    pub token: String,

    #[arg(short, long, default_value = "x86_64-unknown-linux-gnu")]
    pub target: String,

    #[arg(short, long, default_value = "c")]
    pub language: String,

    #[arg(long, default_value = "2")]
    pub optimization: u8,

    #[arg(value_name = "SOURCE_DIR")]
    pub path: PathBuf,
}