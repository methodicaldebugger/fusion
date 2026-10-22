//! Optional HTTP build-artifact cache. The endpoint serves/accepts a raw artifact at /<key>.
use std::{fs, path::Path, process::Command};
pub fn restore(base:&str,key:&str,dest:&Path)->Result<bool,String>{
 let url=format!("{}/{}",base.trim_end_matches('/'),key); let parent=dest.parent().ok_or("artifact destination has no parent")?;fs::create_dir_all(parent).map_err(|e|e.to_string())?;
 let status=Command::new("curl").args(["--fail","--silent","--show-error","--location","--output"]).arg(dest).arg(&url).status().map_err(|e|format!("start curl for remote cache: {e}"))?;
 if status.success(){
  #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let mut permissions=fs::metadata(dest).map_err(|e|e.to_string())?.permissions(); permissions.set_mode(0o755); fs::set_permissions(dest,permissions).map_err(|e|e.to_string())?; }
  Ok(true)
 }else{let _=fs::remove_file(dest);Ok(false)}
}
pub fn publish(base:&str,key:&str,src:&Path)->Result<(),String>{
 let url=format!("{}/{}",base.trim_end_matches('/'),key);let status=Command::new("curl").args(["--fail","--silent","--show-error","--request","PUT","--upload-file"]).arg(src).arg(&url).status().map_err(|e|format!("start curl for remote cache: {e}"))?;
 if status.success(){Ok(())}else{Err(format!("remote cache upload failed for {url}"))}
}
pub fn copy_local(src:&Path,dest:&Path)->Result<(),String>{if let Some(p)=dest.parent(){fs::create_dir_all(p).map_err(|e|e.to_string())?;}fs::copy(src,dest).map(|_|()).map_err(|e|e.to_string())}
