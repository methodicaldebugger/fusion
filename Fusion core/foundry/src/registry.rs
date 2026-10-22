//! Minimal, deterministic Foundry registry client.
//! Registry layout: packages/<name>/<version>.toml with name, version, archive, checksum.
use sha2::{Digest, Sha256};
use std::{fs, path::{Path, PathBuf}, process::Command};

#[derive(Debug, Clone)]
pub struct ResolvedPackage { pub name: String, pub requested: String, pub version: String, pub source: String, pub checksum: Option<String> }

fn numbers(v: &str) -> Option<(u64,u64,u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(|c| c == '-' || c == '+').next()?;
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?; let b = it.next().unwrap_or("0").parse().ok()?; let c = it.next().unwrap_or("0").parse().ok()?;
    if it.next().is_some() { return None; } Some((a,b,c))
}
pub fn is_exact_version(v: &str) -> bool { let t=v.trim(); !t.is_empty() && !t.starts_with('^') && !t.starts_with('~') && !t.starts_with('>') && !t.starts_with('<') && !t.starts_with('=') && !t.contains('-') && !t.contains('+') && !t.contains('*') && !t.contains(' ') && t.split('.').count()==3 && numbers(t).is_some() }
fn cmp(a:(u64,u64,u64), b:(u64,u64,u64))->std::cmp::Ordering { a.cmp(&b) }
pub fn satisfies(version:&str, req:&str)->bool {
    let Some(v)=numbers(version) else{return false}; let r=req.trim();
    if r.is_empty() || r=="*" { return true; }
    if let Some(x)=r.strip_prefix(">=") { return numbers(x).map(|x|cmp(v,x)!=std::cmp::Ordering::Less).unwrap_or(false); }
    if let Some(x)=r.strip_prefix(">") { return numbers(x).map(|x|cmp(v,x)==std::cmp::Ordering::Greater).unwrap_or(false); }
    if let Some(x)=r.strip_prefix("<=") { return numbers(x).map(|x|cmp(v,x)!=std::cmp::Ordering::Greater).unwrap_or(false); }
    if let Some(x)=r.strip_prefix("<") { return numbers(x).map(|x|cmp(v,x)==std::cmp::Ordering::Less).unwrap_or(false); }
    let (op, x)=if let Some(x)=r.strip_prefix('^'){("^",x)}else if let Some(x)=r.strip_prefix('~'){("~",x)}else if let Some(x)=r.strip_prefix('='){("=",x)}else if let Some(x)=r.strip_prefix('v'){("=",x)}else{("^",r)};
    let Some(w)=numbers(x) else { return false };
    match op { "="=>v==w, "~"=>v>=w && v.0==w.0 && (x.split('.').count()<2 || v.1==w.1), _=> v>=w && if w.0>0 {v.0==w.0} else if w.1>0 {v.0==0 && v.1==w.1} else {v.0==0 && v.1==0 && v.2==w.2} }
}
fn fetch_text(url:&str, offline:bool)->Result<String,String>{
    if let Some(path)=url.strip_prefix("file://") { return fs::read_to_string(path).map_err(|e|format!("read registry metadata {path}: {e}")); }
    if !url.contains("://") { return fs::read_to_string(url).map_err(|e|format!("read registry metadata {url}: {e}")); }
    if offline { return Err(format!("registry metadata is not cached for {url} (--offline)")); }
    let out=Command::new("curl").args(["--fail","--silent","--show-error","--location",url]).output().map_err(|e|format!("start curl for registry: {e}"))?;
    if !out.status.success(){return Err(format!("registry request failed for {url}: {}",String::from_utf8_lossy(&out.stderr)));}
    String::from_utf8(out.stdout).map_err(|e|format!("registry metadata is not UTF-8: {e}"))
}
fn download(url:&str,dst:&Path,offline:bool)->Result<(),String>{
    if let Some(path)=url.strip_prefix("file://") { fs::copy(path,dst).map_err(|e|format!("copy package archive: {e}"))?; return Ok(()); }
    if !url.contains("://") { fs::copy(url,dst).map_err(|e|format!("copy package archive: {e}"))?; return Ok(()); }
    if offline{return Err(format!("package archive is not cached: {url}"));}
    let status=Command::new("curl").args(["--fail","--silent","--show-error","--location","--output"]).arg(dst).arg(url).status().map_err(|e|format!("start curl for package: {e}"))?;
    if !status.success(){return Err(format!("download failed for {url}"));} Ok(())
}
pub fn resolve(name:&str,req:&str,registry:&str,cache:&Path,offline:bool)->Result<ResolvedPackage,String>{
    // A requirement can be an exact version or a semver range. A registry's versions.txt
    // contains one version per line; the highest satisfying version is selected.
    let base=registry.trim_end_matches('/');
    let versions_url=format!("{base}/packages/{name}/versions.txt");
    fs::create_dir_all(cache).map_err(|e|e.to_string())?;
    let versions_cache=cache.join(format!("registry-index-{name}-versions.txt"));
    let versions=if offline { fs::read_to_string(&versions_cache).map_err(|_|format!("registry version index for '{name}' is not cached (--offline)"))? } else { let v=fetch_text(&versions_url,false)?; fs::write(&versions_cache,&v).map_err(|e|e.to_string())?; v };
    let mut choices=versions.lines().map(str::trim).filter(|v|!v.is_empty() && satisfies(v,req)).collect::<Vec<_>>();
    choices.sort_by(|a,b| numbers(a).cmp(&numbers(b)));
    let version=choices.last().ok_or_else(||format!("no registry version of '{name}' satisfies '{req}'"))?.to_string();
    let meta_url=format!("{base}/packages/{name}/{version}.toml");
    let metadata_cache=cache.join(format!("registry-meta-{name}-{version}.toml"));
    let meta_text=if offline { fs::read_to_string(&metadata_cache).map_err(|_|format!("registry metadata for {name} {version} is not cached (--offline)"))? } else { let v=fetch_text(&meta_url,false)?; fs::write(&metadata_cache,&v).map_err(|e|e.to_string())?; v };
    let meta:toml::Value=toml::from_str(&meta_text).map_err(|e|format!("invalid registry metadata for {name} {version}: {e}"))?;
    let actual_name=meta.get("name").and_then(toml::Value::as_str).unwrap_or(name);
    let actual_version=meta.get("version").and_then(toml::Value::as_str).unwrap_or(&version);
    if actual_name!=name || actual_version!=version {return Err(format!("registry metadata identity mismatch for {name} {version}"));}
    let archive=meta.get("archive").and_then(toml::Value::as_str).ok_or_else(||format!("registry metadata for {name} {version} lacks archive"))?;
    let checksum=meta.get("checksum").and_then(toml::Value::as_str).ok_or_else(||format!("registry metadata for {name} {version} lacks SHA-256 checksum"))?.to_ascii_lowercase();
    if checksum.len()!=64 || !checksum.bytes().all(|b|b.is_ascii_hexdigit()){return Err(format!("invalid SHA-256 checksum for {name} {version}"));}
    fs::create_dir_all(cache).map_err(|e|e.to_string())?;
    let archive_path=cache.join(format!("{name}-{version}.tar"));
    if !archive_path.is_file() { download(archive,&archive_path,offline)?; }
    let bytes=fs::read(&archive_path).map_err(|e|e.to_string())?;
    let digest=hex::encode(Sha256::digest(&bytes));
    if digest!=checksum { let _=fs::remove_file(&archive_path); return Err(format!("checksum mismatch for {name} {version}: expected {checksum}, got {digest}")); }
    let dest=cache.join(format!("{name}-{version}"));
    if !dest.is_dir(){fs::create_dir_all(&dest).map_err(|e|e.to_string())?;let status=Command::new("tar").arg("-xf").arg(&archive_path).arg("-C").arg(&dest).status().map_err(|e|format!("start tar: {e}"))?;if !status.success(){return Err(format!("could not extract {name} {version}"));}}
    Ok(ResolvedPackage{name:name.into(),requested:req.into(),version,source:meta_url,checksum:Some(checksum)})
}
