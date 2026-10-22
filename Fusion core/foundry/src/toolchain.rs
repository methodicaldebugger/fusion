//! Pinned compiler installer. Manifest pins are exact versions and metadata is supplied by a toolchain feed.
use sha2::{Digest,Sha256};use std::{fs,path::{Path,PathBuf},process::Command};
#[derive(Debug,Clone)]pub struct ToolchainSpec{pub version:String,pub url:String,pub sha256:String,pub executable:String}
pub fn install(spec:&ToolchainSpec,root:&Path,offline:bool)->Result<PathBuf,String>{
 if spec.version.trim().is_empty()||spec.sha256.len()!=64||!spec.sha256.bytes().all(|b|b.is_ascii_hexdigit()){return Err("toolchain requires an exact version and 64-digit SHA-256 checksum".into());}
 let dir=root.join(&spec.version);let exe=dir.join(&spec.executable);if exe.is_file(){return Ok(exe)}if offline{return Err(format!("pinned Fusion toolchain {} is not installed (--offline)",spec.version));}
 fs::create_dir_all(root).map_err(|e|e.to_string())?;let archive=root.join(format!("toolchain-{}.tar",spec.version));
 let status=Command::new("curl").args(["--fail","--silent","--show-error","--location","--output"]).arg(&archive).arg(&spec.url).status().map_err(|e|format!("start curl for toolchain: {e}"))?;if !status.success(){return Err(format!("download failed for Fusion toolchain {}",spec.version));}
 let bytes=fs::read(&archive).map_err(|e|e.to_string())?;let got=hex::encode(Sha256::digest(bytes));if got!=spec.sha256.to_ascii_lowercase(){let _=fs::remove_file(&archive);return Err(format!("toolchain checksum mismatch: expected {}, got {got}",spec.sha256));}
 fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let status=Command::new("tar").args(["-xf"]).arg(&archive).arg("-C").arg(&dir).status().map_err(|e|format!("start tar: {e}"))?;if !status.success(){return Err("failed to extract Fusion toolchain archive".into());}if !exe.is_file(){return Err(format!("toolchain archive did not contain expected executable {}",exe.display()));}Ok(exe)
}
