//! Minimal, deterministic Foundry registry client.
//! Registry layout: packages/<name>/<version>.toml with name, version, archive, checksum.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone)]
pub struct ResolvedPackage { pub name: String, pub requested: String, pub version: String, pub source: String, pub registry_url: String, pub checksum: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachePackageInspection {
    pub name: String,
    pub version: String,
    pub status: String,
    pub archive_bytes: Option<u64>,
    pub detail: String,
}

/// Inspect registry cache records without modifying them. The status is derived
/// from the registry metadata checksum and the extracted-tree integrity sidecar.
pub fn inspect_cache(cache: &Path) -> Result<Vec<CachePackageInspection>, String> {
    if !cache.is_dir() { return Ok(Vec::new()); }
    let mut reports = Vec::new();
    for entry in fs::read_dir(cache).map_err(|error| format!("read registry cache {}: {error}", cache.display()))? {
        let entry = entry.map_err(|error| format!("read registry cache entry: {error}"))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else { continue };
        let Some(stem) = file_name.strip_prefix("registry-meta-").and_then(|value| value.strip_suffix(".toml")) else { continue };
        let metadata_path = entry.path();
        let metadata_kind = fs::symlink_metadata(&metadata_path)
            .map_err(|error| format!("inspect registry metadata {}: {error}", metadata_path.display()))?;
        if !metadata_kind.file_type().is_file() {
            reports.push(CachePackageInspection { name: stem.to_owned(), version: "?".into(), status: "metadata-not-regular-file".into(), archive_bytes: None, detail: "registry metadata entry is not a regular file".into() });
            continue;
        }
        let metadata_text = match fs::read_to_string(&metadata_path) {
            Ok(value) => value,
            Err(error) => {
                reports.push(CachePackageInspection { name: stem.to_owned(), version: "?".into(), status: "metadata-unreadable".into(), archive_bytes: None, detail: error.to_string() });
                continue;
            }
        };
        let metadata: toml::Value = match toml::from_str(&metadata_text) {
            Ok(value) => value,
            Err(error) => {
                reports.push(CachePackageInspection { name: stem.to_owned(), version: "?".into(), status: "metadata-invalid".into(), archive_bytes: None, detail: error.to_string() });
                continue;
            }
        };
        let name = metadata.get("name").and_then(toml::Value::as_str).unwrap_or("?").to_owned();
        let version = metadata.get("version").and_then(toml::Value::as_str).unwrap_or("?").to_owned();
        let checksum = metadata.get("checksum").and_then(toml::Value::as_str).unwrap_or("").to_ascii_lowercase();
        let schema_valid = metadata.get("format_version").and_then(toml::Value::as_integer) == Some(1)
            && metadata.get("archive").and_then(toml::Value::as_str)
                .is_some_and(|archive| validate_archive_location(archive).is_ok());
        let identity_valid = safe_cache_component(&name, "name").is_ok()
            && safe_cache_component(&version, "version").is_ok()
            && stem == format!("{name}-{version}");
        let archive = cache.join(format!("{name}-{version}.tar"));
        let extracted = cache.join(format!("{name}-{version}"));
        let tree_manifest = cache.join(format!("{name}-{version}.tree.toml"));
        let mut status = "healthy";
        let mut detail = "archive checksum and extracted tree are valid".to_owned();
        let mut archive_bytes = None;
        if !schema_valid {
            status = "metadata-schema-invalid";
            detail = "metadata format version or archive location is missing, unsupported, or invalid".into();
        } else if !identity_valid {
            status = "metadata-identity-invalid";
            detail = "metadata filename and package identity do not agree, or contain unsafe components".into();
        } else if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            status = "checksum-invalid";
            detail = "metadata does not contain a valid SHA-256 checksum".into();
        } else {
            let archive_kind = fs::symlink_metadata(&archive);
            if archive_kind.as_ref().is_ok_and(|metadata| !metadata.file_type().is_file()) {
                status = "archive-not-regular-file";
                detail = "cached archive is not a regular file".into();
            } else { match fs::read(&archive) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    status = "archive-missing";
                    detail = "verified package archive is not cached".into();
                }
                Err(error) => {
                    status = "archive-unreadable";
                    detail = error.to_string();
                }
                Ok(bytes) => {
                    archive_bytes = Some(bytes.len() as u64);
                    let actual = hex::encode(Sha256::digest(&bytes));
                    if actual != checksum {
                        status = "archive-corrupt";
                        detail = format!("archive SHA-256 mismatch (expected {checksum}, got {actual})");
                    } else if !extracted.is_dir() {
                        status = "extracted-missing";
                        detail = "archive is valid but extracted package directory is missing".into();
                    } else if let Some(expected) = read_tree_manifest(&tree_manifest, &checksum) {
                        match package_tree_sha256(&extracted) {
                            Ok(actual) if actual == expected => {}
                            Ok(actual) => {
                                status = "tree-corrupt";
                                detail = format!("extracted tree SHA-256 mismatch (expected {expected}, got {actual})");
                            }
                            Err(error) => {
                                status = "tree-invalid";
                                detail = error;
                            }
                        }
                    } else {
                        status = "tree-manifest-missing-or-invalid";
                        detail = "extracted tree integrity sidecar is missing, malformed, or belongs to another archive".into();
                    }
                }
            }}
        }
        reports.push(CachePackageInspection { name, version, status: status.into(), archive_bytes, detail });
    }
    reports.sort_by(|left, right| (&left.name, &left.version).cmp(&(&right.name, &right.version)));
    Ok(reports)
}

fn numbers(v: &str) -> Option<(u64,u64,u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(|c| c == '-' || c == '+').next()?;
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?; let b = it.next().unwrap_or("0").parse().ok()?; let c = it.next().unwrap_or("0").parse().ok()?;
    if it.next().is_some() { return None; } Some((a,b,c))
}
pub fn is_exact_version(v: &str) -> bool { let t=v.trim(); !t.is_empty() && !t.starts_with('^') && !t.starts_with('~') && !t.starts_with('>') && !t.starts_with('<') && !t.starts_with('=') && !t.contains('-') && !t.contains('+') && !t.contains('*') && !t.contains(' ') && t.split('.').count()==3 && numbers(t).is_some() }
fn exact_version_requirement(requirement: &str) -> Option<String> {
    let trimmed = requirement.trim();
    let candidate = trimmed.strip_prefix('=').unwrap_or(trimmed);
    let normalized = candidate.strip_prefix('v').unwrap_or(candidate);
    is_exact_version(normalized).then(|| normalized.to_owned())
}
fn cmp(a:(u64,u64,u64), b:(u64,u64,u64))->std::cmp::Ordering { a.cmp(&b) }
fn satisfies_single(version: &str, requirement: &str) -> bool {
    let Some(v) = numbers(version) else { return false };
    let r = requirement.trim();
    if r.is_empty() || r == "*" { return true; }
    if let Some(x) = r.strip_prefix(">=") { return numbers(x).map(|x| cmp(v, x) != std::cmp::Ordering::Less).unwrap_or(false); }
    if let Some(x) = r.strip_prefix(">") { return numbers(x).map(|x| cmp(v, x) == std::cmp::Ordering::Greater).unwrap_or(false); }
    if let Some(x) = r.strip_prefix("<=") { return numbers(x).map(|x| cmp(v, x) != std::cmp::Ordering::Greater).unwrap_or(false); }
    if let Some(x) = r.strip_prefix("<") { return numbers(x).map(|x| cmp(v, x) == std::cmp::Ordering::Less).unwrap_or(false); }
    let (op, x) = if let Some(x) = r.strip_prefix('^') { ("^", x) }
        else if let Some(x) = r.strip_prefix('~') { ("~", x) }
        else if let Some(x) = r.strip_prefix('=') { ("=", x) }
        else if let Some(x) = r.strip_prefix('v') { ("=", x) }
        else { ("^", r) };
    let Some(w) = numbers(x) else { return false };
    match op {
        "=" => v == w,
        "~" => v >= w && v.0 == w.0 && (x.split('.').count() < 2 || v.1 == w.1),
        _ => v >= w && if w.0 > 0 { v.0 == w.0 } else if w.1 > 0 { v.0 == 0 && v.1 == w.1 } else { v.0 == 0 && v.1 == 0 && v.2 == w.2 },
    }
}

/// Checks a version against a comma-separated conjunction of requirements,
/// e.g. `>=1.4.0, <2.0.0`. Every clause must match.
pub fn satisfies(version: &str, requirement: &str) -> bool {
    let normalized = requirement.replace(',', " ");
    normalized.split_whitespace().all(|clause| satisfies_single(version, clause))
}

/// One package requirement contributed by a node in the dependency graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionConstraint {
    pub requester: String,
    pub requirement: String,
}

/// Selects the highest available version satisfying every workspace constraint.
/// Requester names are retained in conflict diagnostics to make graph conflicts actionable.
pub fn select_version_for_constraints(
    package: &str,
    constraints: &[VersionConstraint],
    available_versions: &[String],
) -> Result<String, String> {
    if constraints.is_empty() {
        return Err(format!("no version constraints were supplied for '{package}'"));
    }
    let mut candidates = available_versions.iter()
        .filter(|version| numbers(version).is_some())
        .filter(|version| constraints.iter().all(|constraint| satisfies(version, &constraint.requirement)))
        .cloned().collect::<Vec<_>>();
    candidates.sort_by(|a, b| numbers(a).cmp(&numbers(b)));
    if let Some(version) = candidates.pop() { return Ok(version); }
    let details = constraints.iter()
        .map(|constraint| format!("{} requires {}", constraint.requester, constraint.requirement))
        .collect::<Vec<_>>().join("; ");
    Err(format!("workspace version conflict for '{package}': no published version satisfies all constraints ({details})"))
}
fn validate_registry_base(registry: &str) -> Result<String, String> {
    let base = registry.trim().trim_end_matches('/');
    if base.is_empty() || base.bytes().any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace()) {
        return Err("registry location is empty or contains whitespace/control characters".into());
    }
    if base.starts_with("https://") {
        let authority = base[8..].split('/').next().unwrap_or("");
        if authority.is_empty() || authority.contains('@') || authority.starts_with('.') {
            return Err("registry HTTPS URL must have a valid host and must not contain user information".into());
        }
        if base.contains('#') || base.contains('?') {
            return Err("registry base URL must not contain a query string or fragment".into());
        }
        return Ok(base.to_owned());
    }
    if let Some(path) = base.strip_prefix("file://") {
        if !Path::new(path).is_absolute() || (path.contains('?') || path.contains('#')) {
            return Err("file registry URL must reference an absolute local directory without query or fragment".into());
        }
        return Ok(base.to_owned());
    }
    if base.contains("://") {
        return Err("unsupported registry scheme; use HTTPS or a local filesystem directory".into());
    }
    if !Path::new(base).is_absolute() {
        return Err("local registry directory must be an absolute path".into());
    }
    Ok(base.to_owned())
}

fn validate_archive_location(archive: &str) -> Result<String, String> {
    if archive.is_empty() || archive.bytes().any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace()) {
        return Err("registry archive location is empty or contains whitespace/control characters".into());
    }
    if archive.starts_with("https://") {
        let authority = archive[8..].split('/').next().unwrap_or("");
        if authority.is_empty() || authority.contains('@') {
            return Err("registry archive HTTPS URL must have a host and must not contain user information".into());
        }
        if archive.contains('#') {
            return Err("registry archive URL must not contain a fragment".into());
        }
        return Ok(archive.to_owned());
    }
    if let Some(path) = archive.strip_prefix("file://") {
        if !Path::new(path).is_absolute() || (path.contains('?') || path.contains('#')) {
            return Err("file archive URL must reference an absolute local file without query or fragment".into());
        }
        return Ok(archive.to_owned());
    }
    if archive.contains("://") {
        return Err("unsupported archive scheme; registry archives must use HTTPS or a local file".into());
    }
    if !Path::new(archive).is_absolute() {
        return Err("registry archive must be an absolute path, file:// URL, or HTTPS URL".into());
    }
    Ok(archive.to_owned())
}

fn write_atomic(path: &Path, contents: &[u8], label: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("{label} destination has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| format!("create {label} directory: {error}"))?;
    let temp = unique_sibling(parent, "registry-state", "tmp");
    let result = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)
            .map_err(|error| format!("create temporary {label}: {error}"))?;
        file.write_all(contents).map_err(|error| format!("write {label}: {error}"))?;
        file.sync_all().map_err(|error| format!("sync {label}: {error}"))?;
        fs::rename(&temp, path).map_err(|error| format!("publish {label}: {error}"))?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result
}

fn fetch_text(url:&str, offline:bool)->Result<String,String>{
    if let Some(path)=url.strip_prefix("file://") { return fs::read_to_string(path).map_err(|e|format!("read local registry metadata {path}: {e}")); }
    if !url.contains("://") { return fs::read_to_string(url).map_err(|e|format!("read local registry metadata {url}: {e}")); }
    if !url.starts_with("https://") { return Err("registry network requests require HTTPS".into()); }
    if offline { return Err(format!("registry metadata is not cached for {url} (--offline)")); }
    let out=Command::new("curl").args(["--fail","--silent","--show-error","--location","--proto","=https","--proto-redir","=https","--connect-timeout","15","--max-time","120","--retry","2",url]).output().map_err(|e|format!("could not start HTTPS registry request: {e}"))?;
    if !out.status.success(){return Err(format!("HTTPS registry request failed (curl status {}): {}",out.status,String::from_utf8_lossy(&out.stderr).trim()));}
    String::from_utf8(out.stdout).map_err(|e|format!("registry metadata response is not UTF-8: {e}"))
}
fn download(url:&str,dst:&Path,offline:bool)->Result<(),String>{
    if offline{return Err(format!("package archive is not cached: {url}"));}
    let url = validate_archive_location(url)?;
    let parent = dst.parent().ok_or_else(|| "package archive destination has no parent".to_string())?;
    let temp = unique_sibling(parent, "registry-archive", "download-tmp");
    let result = (|| {
        if let Some(path)=url.strip_prefix("file://") {
            fs::copy(path,&temp).map_err(|e|format!("copy local package archive: {e}"))?;
        } else if !url.contains("://") {
            fs::copy(&url,&temp).map_err(|e|format!("copy local package archive: {e}"))?;
        } else {
            let status=Command::new("curl").args(["--fail","--silent","--show-error","--location","--proto","=https","--proto-redir","=https","--connect-timeout","15","--max-time","300","--retry","2","--output"]).arg(&temp).arg(&url).status().map_err(|e|format!("could not start HTTPS package download: {e}"))?;
            if !status.success(){return Err(format!("HTTPS package download failed (curl status {status}) for {url}"));}
        }
        let file = fs::OpenOptions::new().read(true).write(true).open(&temp).map_err(|e|format!("open downloaded package archive: {e}"))?;
        file.sync_all().map_err(|e|format!("sync downloaded package archive: {e}"))?;
        fs::rename(&temp,dst).map_err(|e|format!("publish downloaded package archive: {e}"))?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result
}

fn validate_registry_metadata(text: &str, expected_name: &str, expected_version: &str) -> Result<(String, String), String> {
    let metadata: toml::Value = toml::from_str(text)
        .map_err(|error| format!("invalid registry metadata for {expected_name} {expected_version}: {error}"))?;
    let format_version = metadata.get("format_version").and_then(toml::Value::as_integer)
        .ok_or_else(|| format!("registry metadata for {expected_name} {expected_version} is missing integer format_version"))?;
    if format_version != 1 {
        return Err(format!("unsupported registry metadata format_version {format_version} for {expected_name} {expected_version}; supported format is 1"));
    }
    let actual_name = metadata.get("name").and_then(toml::Value::as_str)
        .ok_or_else(|| format!("registry metadata for {expected_name} {expected_version} lacks string name"))?;
    let actual_version = metadata.get("version").and_then(toml::Value::as_str)
        .ok_or_else(|| format!("registry metadata for {expected_name} {expected_version} lacks string version"))?;
    if actual_name != expected_name || actual_version != expected_version {
        return Err(format!("registry metadata identity mismatch: expected {expected_name} {expected_version}, received {actual_name} {actual_version}"));
    }
    let archive = metadata.get("archive").and_then(toml::Value::as_str)
        .ok_or_else(|| format!("registry metadata for {expected_name} {expected_version} lacks string archive"))?;
    let archive = validate_archive_location(archive)
        .map_err(|error| format!("invalid archive location for {expected_name} {expected_version}: {error}"))?;
    let checksum = metadata.get("checksum").and_then(toml::Value::as_str)
        .ok_or_else(|| format!("registry metadata for {expected_name} {expected_version} lacks string SHA-256 checksum"))?
        .to_ascii_lowercase();
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("invalid SHA-256 checksum for {expected_name} {expected_version}"));
    }
    Ok((archive, checksum))
}

fn safe_cache_component(value: &str, kind: &str) -> Result<(), String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
    {
        return Err(format!("invalid registry package {kind} '{value}'"));
    }
    Ok(())
}

fn unique_sibling(parent: &Path, stem: &str, suffix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    parent.join(format!(".{stem}-{}-{nonce}.{suffix}", std::process::id()))
}


/// Cross-process, per-package cache lock. A lock is removed automatically only
/// when its recorded owner is demonstrably no longer running; ambiguous states
/// fail closed rather than risking two writers publishing the same cache entry.
struct RegistryPackageLock {
    path: PathBuf,
}

impl RegistryPackageLock {
    fn acquire(cache: &Path, name: &str) -> Result<Self, String> {
        let path = cache.join(format!(".registry-lock-{name}"));
        let started = SystemTime::now();
        loop {
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let record = format!("pid={}\n", std::process::id());
                    if let Err(error) = file.write_all(record.as_bytes()).and_then(|_| file.sync_all()) {
                        let _ = fs::remove_file(&path);
                        return Err(format!("write registry cache lock {}: {error}", path.display()));
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let owner = fs::read_to_string(&path).ok()
                        .and_then(|contents| contents.trim().strip_prefix("pid=")?.parse::<u32>().ok());
                    if let Some(pid) = owner {
                        if !process_is_running(pid) {
                            // Serialize stale-lock reclamation, then re-read the
                            // owner under that guard so a newly acquired live
                            // lock can never be removed by a competing reaper.
                            if reclaim_stale_registry_lock(&path, name)? { continue; }
                        }
                    }
                    if started.elapsed().unwrap_or_default() > Duration::from_secs(120) {
                        return Err(format!("timed out waiting for registry cache lock {}; if its owner has crashed, remove the stale lock file", path.display()));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(format!("create registry cache lock {}: {error}", path.display())),
            }
        }
    }
}

impl Drop for RegistryPackageLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn reclaim_stale_registry_lock(path: &Path, name: &str) -> Result<bool, String> {
    let reaper_path = path.with_file_name(format!(".registry-reap-{name}"));
    let mut reaper = match fs::OpenOptions::new().write(true).create_new(true).open(&reaper_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // A process may have crashed while reclaiming a stale package lock.
            // Recover a reaper only when its recorded PID is demonstrably gone;
            // re-read the exact record immediately before removal so a newer
            // live reaper cannot be mistaken for the abandoned one.
            let previous = fs::read(&reaper_path).ok();
            let stale_pid = previous.as_deref().and_then(|bytes| {
                std::str::from_utf8(bytes).ok()?.trim().strip_prefix("pid=")?.parse::<u32>().ok()
            });
            if let (Some(previous), Some(pid)) = (previous, stale_pid) {
                if !process_is_running(pid) && fs::read(&reaper_path).ok().as_deref() == Some(previous.as_slice()) {
                    let still_stale = fs::read_to_string(&reaper_path).ok()
                        .and_then(|contents| contents.trim().strip_prefix("pid=")?.parse::<u32>().ok())
                        .is_some_and(|current| current == pid && !process_is_running(current));
                    if still_stale { let _ = fs::remove_file(&reaper_path); }
                }
            }
            return Ok(false);
        }
        Err(error) => return Err(format!("create registry stale-lock reaper {}: {error}", reaper_path.display())),
    };
    let result = (|| {
        writeln!(reaper, "pid={}", std::process::id()).map_err(|error| format!("write registry stale-lock reaper: {error}"))?;
        reaper.sync_all().map_err(|error| format!("sync registry stale-lock reaper: {error}"))?;
        let current_owner = fs::read_to_string(path).ok()
            .and_then(|contents| contents.trim().strip_prefix("pid=")?.parse::<u32>().ok());
        if current_owner.is_some_and(|pid| !process_is_running(pid)) {
            match fs::remove_file(path) {
                Ok(()) => return Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
                Err(error) => return Err(format!("remove stale registry cache lock {}: {error}", path.display())),
            }
        }
        Ok(false)
    })();
    drop(reaper);
    let _ = fs::remove_file(&reaper_path);
    result
}

fn process_is_running(pid: u32) -> bool {
    if pid == 0 { return false; }
    #[cfg(unix)]
    {
        return Command::new("kill").arg("-0").arg(pid.to_string()).status()
            .map(|status| status.success()).unwrap_or(true);
    }
    #[cfg(windows)]
    {
        let filter = format!("PID eq {pid}");
        return Command::new("tasklist").args(["/FI", filter.as_str(), "/NH"]).output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
            .unwrap_or(true);
    }
    #[allow(unreachable_code)]
    true
}

fn cleanup_package_transaction_artifacts(cache: &Path, name: &str, version: &str) {
    let prefix = format!(".{name}-{version}-");
    let destination = cache.join(format!("{name}-{version}"));
    let manifest = cache.join(format!("{name}-{version}.tree.toml"));
    // Called only while holding the package lock and after a verified destination
    // has been published, so abandoned transaction siblings are no longer needed.
    if !destination.is_dir() || !manifest.is_file() { return; }
    let Ok(entries) = fs::read_dir(cache) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else { continue };
        if file_name.starts_with(&prefix)
            && [".extract", ".backup", ".manifest-backup", ".tree-tmp"].iter().any(|suffix| file_name.ends_with(suffix))
        {
            remove_cached_path(&path);
        }
    }
}


pub(crate) fn safe_registry_cache_component(value: &str) -> Result<(), String> {
    safe_cache_component(value, "cache component")
}

pub(crate) fn lock_package_cache(cache: &Path, name: &str) -> Result<impl Drop, String> {
    RegistryPackageLock::acquire(cache, name)
}

pub(crate) fn lock_workspace_cache(cache: &Path) -> Result<impl Drop, String> {
    RegistryPackageLock::acquire(cache, "__workspace__")
}

fn archive_entries_are_safe(archive: &Path) -> Result<(), String> {
    let listing = Command::new("tar")
        .arg("-tf")
        .arg(archive)
        .output()
        .map_err(|error| format!("start tar archive inspection: {error}"))?;
    if !listing.status.success() {
        return Err(format!("could not inspect package archive {}", archive.display()));
    }
    let names = String::from_utf8(listing.stdout)
        .map_err(|error| format!("package archive listing is not UTF-8: {error}"))?;
    for name in names.lines() {
        let trimmed = name.trim_end_matches('/');
        if trimmed.is_empty() { continue; }
        if trimmed.contains('\\') {
            return Err(format!("package archive contains an unsafe path '{name}'"));
        }
        let path = Path::new(trimmed);
        if path.is_absolute()
            || path.components().any(|component| matches!(component, Component::ParentDir | Component::RootDir))
            || path.components().any(|component| matches!(component, Component::Normal(part) if part.to_string_lossy().ends_with(':')))
        {
            return Err(format!("package archive contains an unsafe path '{name}'"));
        }
    }

    // Reject links and special files. A symlink inside an archive can redirect
    // later extraction entries outside the package directory.
    let verbose = Command::new("tar")
        .arg("-tvf")
        .arg(archive)
        .output()
        .map_err(|error| format!("start tar archive type inspection: {error}"))?;
    if !verbose.status.success() {
        return Err(format!("could not inspect package archive entry types for {}", archive.display()));
    }
    let entries = String::from_utf8(verbose.stdout)
        .map_err(|error| format!("package archive details are not UTF-8: {error}"))?;
    for line in entries.lines() {
        let entry_type = line.as_bytes().first().copied();
        if !matches!(entry_type, Some(b'-' | b'd')) {
            return Err(format!("package archive contains a link or unsupported special entry: {line}"));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn permission_identity(metadata: &fs::Metadata) -> u32 {
    metadata.permissions().mode() & 0o777
}
#[cfg(not(unix))]
fn permission_identity(metadata: &fs::Metadata) -> u32 {
    if metadata.permissions().readonly() { 1 } else { 0 }
}

fn package_tree_sha256(root: &Path) -> Result<String, String> {
    let root_metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("inspect extracted package {}: {error}", root.display()))?;
    if !root_metadata.file_type().is_dir() || root_metadata.file_type().is_symlink() {
        return Err(format!("extracted package {} is not a real directory", root.display()));
    }

    fn collect(root: &Path, current: &Path, entries: &mut BTreeMap<String, (u8, u32, Vec<u8>)>) -> Result<(), String> {
        let mut children = fs::read_dir(current)
            .map_err(|error| format!("read extracted package {}: {error}", current.display()))?
            .map(|entry| entry.map(|entry| entry.path()).map_err(|error| error.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        children.sort();
        for path in children {
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("inspect extracted package entry {}: {error}", path.display()))?;
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
            let relative = relative.to_str().ok_or_else(|| format!("non-UTF-8 path in extracted package: {}", path.display()))?.replace('\\', "/");
            if metadata.file_type().is_symlink() {
                return Err(format!("symlink in extracted package is not permitted: {relative}"));
            }
            let mode = permission_identity(&metadata);
            if metadata.is_dir() {
                entries.insert(relative, (b'd', mode, Vec::new()));
                collect(root, &path, entries)?;
            } else if metadata.is_file() {
                let bytes = fs::read(&path).map_err(|error| format!("read extracted package file {}: {error}", path.display()))?;
                entries.insert(relative, (b'f', mode, bytes));
            } else {
                return Err(format!("special file in extracted package is not permitted: {relative}"));
            }
        }
        Ok(())
    }

    let mut entries = BTreeMap::new();
    collect(root, root, &mut entries)?;
    let mut digest = Sha256::new();
    for (path, (kind, mode, bytes)) in entries {
        digest.update([kind]);
        digest.update((path.len() as u64).to_le_bytes());
        digest.update(path.as_bytes());
        digest.update(mode.to_le_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    Ok(hex::encode(digest.finalize()))
}

fn read_tree_manifest(path: &Path, archive_sha256: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let value: toml::Value = toml::from_str(&text).ok()?;
    if value.get("format_version")?.as_integer()? != 1
        || value.get("archive_sha256")?.as_str()? != archive_sha256
    {
        return None;
    }
    let tree = value.get("tree_sha256")?.as_str()?;
    (tree.len() == 64 && tree.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| tree.to_ascii_lowercase())
}

fn remove_cached_path(path: &Path) {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => { let _ = fs::remove_dir_all(path); }
        Ok(_) => { let _ = fs::remove_file(path); }
        Err(_) => {}
    }
}

fn extract_verified_package(
    name: &str,
    version: &str,
    archive_path: &Path,
    archive_sha256: &str,
    destination: &Path,
    manifest_path: &Path,
) -> Result<(), String> {
    archive_entries_are_safe(archive_path)?;
    let parent = destination.parent().ok_or_else(|| "package cache has no parent directory".to_string())?;
    let stem = format!("{name}-{version}");
    let staging = unique_sibling(parent, &stem, "extract");
    let backup = unique_sibling(parent, &stem, "backup");
    let manifest_backup = unique_sibling(parent, &stem, "manifest-backup");
    fs::create_dir(&staging).map_err(|error| format!("create package extraction staging directory: {error}"))?;

    let extraction = Command::new("tar")
        .arg("-xf").arg(archive_path)
        .arg("-C").arg(&staging)
        .status()
        .map_err(|error| format!("start tar: {error}"));
    let extraction = match extraction {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(format!("could not extract {name} {version}")),
        Err(error) => Err(error),
    };
    if let Err(error) = extraction { let _ = fs::remove_dir_all(&staging); return Err(error); }

    let tree_sha256 = match package_tree_sha256(&staging) {
        Ok(value) => value,
        Err(error) => { let _ = fs::remove_dir_all(&staging); return Err(error); }
    };
    let manifest_temp = unique_sibling(parent, &stem, "tree-tmp");
    let manifest_contents = format!(
        "format_version = 1\narchive_sha256 = \"{archive_sha256}\"\ntree_sha256 = \"{tree_sha256}\"\n"
    );
    let manifest_write = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&manifest_temp)
            .map_err(|error| format!("create registry tree manifest: {error}"))?;
        file.write_all(manifest_contents.as_bytes()).map_err(|error| format!("write registry tree manifest: {error}"))?;
        file.sync_all().map_err(|error| format!("sync registry tree manifest: {error}"))?;
        Ok(())
    })();
    if let Err(error) = manifest_write {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_file(&manifest_temp);
        return Err(error);
    }

    let had_destination = fs::symlink_metadata(destination).is_ok();
    let had_manifest = fs::symlink_metadata(manifest_path).is_ok();
    if had_destination {
        if let Err(error) = fs::rename(destination, &backup) {
            let _ = fs::remove_dir_all(&staging); let _ = fs::remove_file(&manifest_temp);
            return Err(format!("stage previous package cache directory: {error}"));
        }
    }
    if had_manifest {
        if let Err(error) = fs::rename(manifest_path, &manifest_backup) {
            if had_destination { let _ = fs::rename(&backup, destination); }
            let _ = fs::remove_dir_all(&staging); let _ = fs::remove_file(&manifest_temp);
            return Err(format!("stage previous package tree manifest: {error}"));
        }
    }

    if let Err(error) = fs::rename(&staging, destination) {
        if had_destination { let _ = fs::rename(&backup, destination); }
        if had_manifest { let _ = fs::rename(&manifest_backup, manifest_path); }
        let _ = fs::remove_dir_all(&staging); let _ = fs::remove_file(&manifest_temp);
        return Err(format!("publish extracted package directory: {error}"));
    }
    if let Err(error) = fs::rename(&manifest_temp, manifest_path) {
        let _ = fs::remove_dir_all(destination);
        if had_destination { let _ = fs::rename(&backup, destination); }
        if had_manifest { let _ = fs::rename(&manifest_backup, manifest_path); }
        let _ = fs::remove_file(&manifest_temp);
        return Err(format!("publish package tree manifest: {error}"));
    }
    if had_destination { remove_cached_path(&backup); }
    if had_manifest { let _ = fs::remove_file(&manifest_backup); }
    Ok(())
}

fn ensure_extracted_package(
    name: &str,
    version: &str,
    archive_path: &Path,
    archive_sha256: &str,
    destination: &Path,
    cache: &Path,
) -> Result<(), String> {
    let manifest_path = cache.join(format!("{name}-{version}.tree.toml"));
    if fs::symlink_metadata(destination).is_ok() {
        if let (Some(expected), Ok(actual)) = (
            read_tree_manifest(&manifest_path, archive_sha256),
            package_tree_sha256(destination),
        ) {
            if expected == actual { return Ok(()); }
        }
    }
    extract_verified_package(name, version, archive_path, archive_sha256, destination, &manifest_path)
}

pub fn resolve(name: &str, req: &str, registry: &str, cache: &Path, offline: bool) -> Result<ResolvedPackage, String> {
    resolve_with_constraints(name, &[VersionConstraint { requester: "manifest".into(), requirement: req.into() }], registry, cache, offline)
}

/// Resolve a registry package against all known workspace requirements. The
/// caller supplies the complete constraint set for this package name.
pub fn resolve_with_constraints(
    name: &str,
    constraints: &[VersionConstraint],
    registry: &str,
    cache: &Path,
    offline: bool,
) -> Result<ResolvedPackage, String> {
    if constraints.is_empty() { return Err(format!("no version constraints were supplied for '{name}'")); }
    safe_cache_component(name, "name")?;
    let requested = constraints.iter().map(|constraint| constraint.requirement.as_str()).collect::<Vec<_>>().join(" & ");
    // A requirement can be an exact version or a semver range. A registry's versions.txt
    // contains one version per line; the highest satisfying version is selected.
    let base=validate_registry_base(registry)?;
    fs::create_dir_all(cache).map_err(|e|e.to_string())?;
    let _package_lock = RegistryPackageLock::acquire(cache, name)?;
    let version = if constraints.len() == 1 {
        if let Some(exact) = exact_version_requirement(&constraints[0].requirement) {
            // A lockfile pin is already an exact resolution. Do not make offline
            // builds depend on a cached (and potentially stale) versions index.
            exact
        } else {
            let versions_url = format!("{base}/packages/{name}/versions.txt");
            let versions_cache = cache.join(format!("registry-index-{name}-versions.txt"));
            let versions = if offline {
                fs::read_to_string(&versions_cache).map_err(|_| format!("registry version index for '{name}' is not cached (--offline)"))?
            } else {
                let value = fetch_text(&versions_url, false)?;
                write_atomic(&versions_cache, value.as_bytes(), "registry version index")?;
                value
            };
            let available = versions.lines().map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned).collect::<Vec<_>>();
            select_version_for_constraints(name, constraints, &available)?
        }
    } else {
        let versions_url = format!("{base}/packages/{name}/versions.txt");
        let versions_cache = cache.join(format!("registry-index-{name}-versions.txt"));
        let versions = if offline {
            fs::read_to_string(&versions_cache).map_err(|_| format!("registry version index for '{name}' is not cached (--offline)"))?
        } else {
            let value = fetch_text(&versions_url, false)?;
            write_atomic(&versions_cache, value.as_bytes(), "registry version index")?;
            value
        };
        let available = versions.lines().map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned).collect::<Vec<_>>();
        select_version_for_constraints(name, constraints, &available)?
    };
    safe_cache_component(&version, "version")?;
    let meta_url=format!("{base}/packages/{name}/{version}.toml");
    let metadata_cache=cache.join(format!("registry-meta-{name}-{version}.toml"));
    let meta_text=if offline {
        fs::read_to_string(&metadata_cache).map_err(|_|format!("registry metadata for {name} {version} is not cached (--offline)"))?
    } else {
        fetch_text(&meta_url,false)?
    };
    let (archive, checksum) = validate_registry_metadata(&meta_text, name, &version)?;
    if !offline {
        write_atomic(&metadata_cache, meta_text.as_bytes(), "registry package metadata")?;
    }
    fs::create_dir_all(cache).map_err(|e|e.to_string())?;
    let archive_path=cache.join(format!("{name}-{version}.tar"));
    if !archive_path.is_file() { download(&archive,&archive_path,offline)?; }
    let bytes=fs::read(&archive_path).map_err(|e|e.to_string())?;
    let digest=hex::encode(Sha256::digest(&bytes));
    if digest!=checksum { let _=fs::remove_file(&archive_path); return Err(format!("checksum mismatch for {name} {version}: expected {checksum}, got {digest}")); }
    let dest=cache.join(format!("{name}-{version}"));
    ensure_extracted_package(name, &version, &archive_path, &checksum, &dest, cache)?;
    cleanup_package_transaction_artifacts(cache, name, &version);
    Ok(ResolvedPackage{name:name.into(),requested,version,source:meta_url,registry_url:base.to_owned(),checksum:Some(checksum)})
}

#[cfg(test)]
mod tests {
    use super::{cleanup_package_transaction_artifacts, exact_version_requirement, process_is_running, resolve, satisfies, select_version_for_constraints, RegistryPackageLock, VersionConstraint};
    use sha2::{Digest, Sha256};
    use std::{fs, process::Command, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn stale_registry_lock_is_reclaimed_when_owner_process_is_gone() {
        let root = std::env::temp_dir().join(format!("foundry-registry-lock-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let lock_path = root.join(".registry-lock-demo");
        fs::write(&lock_path, "pid=4294967294\n").unwrap();
        let lock = RegistryPackageLock::acquire(&root, "demo").unwrap();
        assert!(lock_path.is_file());
        drop(lock);
        assert!(!lock_path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interrupted_stale_lock_reclamation_recovers_a_dead_reaper_record() {
        let root = std::env::temp_dir().join(format!("foundry-registry-reaper-recovery-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let lock_path = root.join(".registry-lock-demo");
        let reaper_path = root.join(".registry-reap-demo");
        fs::write(&lock_path, "pid=4294967294\n").unwrap();
        fs::write(&reaper_path, "pid=4294967293\n").unwrap();
        let lock = RegistryPackageLock::acquire(&root, "demo").unwrap();
        assert!(lock_path.is_file());
        assert!(!reaper_path.exists(), "abandoned reaper record should be reclaimed");
        drop(lock);
        assert!(!lock_path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_lock_reports_current_process_as_running() {
        assert!(process_is_running(std::process::id()));
    }

    #[test]
    fn package_cache_lock_serializes_competing_threads() {
        let root = std::env::temp_dir().join(format!("foundry-registry-contention-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let first = RegistryPackageLock::acquire(&root, "demo").unwrap();
        let worker_root = root.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _second = RegistryPackageLock::acquire(&worker_root, "demo").unwrap();
            sender.send(()).unwrap();
        });
        assert!(receiver.recv_timeout(std::time::Duration::from_millis(150)).is_err());
        drop(first);
        receiver.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn package_transaction_cleanup_removes_only_matching_stale_siblings() {
        let root = std::env::temp_dir().join(format!("foundry-registry-cleanup-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(root.join("demo-1.2.3")).unwrap();
        fs::write(root.join("demo-1.2.3.tree.toml"), "verified").unwrap();
        fs::create_dir_all(root.join(".demo-1.2.3-1-2.extract")).unwrap();
        fs::write(root.join(".demo-1.2.3-1-2.tree-tmp"), "stale").unwrap();
        fs::create_dir_all(root.join(".other-1.2.3-1-2.extract")).unwrap();
        cleanup_package_transaction_artifacts(&root, "demo", "1.2.3");
        assert!(!root.join(".demo-1.2.3-1-2.extract").exists());
        assert!(!root.join(".demo-1.2.3-1-2.tree-tmp").exists());
        assert!(root.join(".other-1.2.3-1-2.extract").exists());
        let _ = fs::remove_dir_all(root);
    }

    fn registry_fixture(label: &str) -> (std::path::PathBuf, std::path::PathBuf, String) {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("foundry-registry-{label}-{nonce}"));
        let cache = root.join("cache");
        let source = root.join("source");
        fs::create_dir_all(source.join("src")).unwrap();
        fs::create_dir_all(&cache).unwrap();
        fs::write(source.join("src/lib.fusion"), "pub const VALUE = 3\n").unwrap();
        let archive_path = cache.join("demo-1.2.3.tar");
        let status = Command::new("tar")
            .arg("-cf").arg(&archive_path)
            .arg("-C").arg(&source)
            .arg("src")
            .status().unwrap();
        assert!(status.success());
        let archive = fs::read(&archive_path).unwrap();
        let checksum = hex::encode(Sha256::digest(&archive));
        let metadata = format!("format_version = 1\nname = \"demo\"\nversion = \"1.2.3\"\narchive = {:?}\nchecksum = \"{checksum}\"\n", archive_path.to_string_lossy().to_string());
        fs::write(cache.join("registry-meta-demo-1.2.3.toml"), &metadata).unwrap();
        let registry_metadata = root.join("packages/demo/1.2.3.toml");
        fs::create_dir_all(registry_metadata.parent().unwrap()).unwrap();
        fs::write(registry_metadata, metadata).unwrap();
        (root, cache, checksum)
    }

    #[test]
    fn concurrent_online_and_offline_resolution_share_one_verified_cache_entry() {
        let (root, cache, _) = registry_fixture("concurrent-online-offline");
        let registry = root.to_string_lossy().to_string();
        let online_cache = cache.clone();
        let online_registry = registry.clone();
        let online = std::thread::spawn(move || resolve("demo", "=1.2.3", &online_registry, &online_cache, false));
        let offline_cache = cache.clone();
        let offline_registry = registry.clone();
        let offline = std::thread::spawn(move || resolve("demo", "=1.2.3", &offline_registry, &offline_cache, true));
        assert_eq!(online.join().unwrap().unwrap().version, "1.2.3");
        assert_eq!(offline.join().unwrap().unwrap().version, "1.2.3");
        assert_eq!(fs::read_to_string(cache.join("demo-1.2.3/src/lib.fusion")).unwrap(), "pub const VALUE = 3\n");
        assert!(cache.join("demo-1.2.3.tree.toml").is_file());
        assert!(!cache.join(".registry-lock-demo").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_inspection_reports_healthy_archive_and_extracted_tree() {
        let (root, cache, _) = registry_fixture("inspect-healthy");
        resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap();
        let reports = super::inspect_cache(&cache).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].name, "demo");
        assert_eq!(reports[0].version, "1.2.3");
        assert_eq!(reports[0].status, "healthy");
        assert!(reports[0].archive_bytes.unwrap() > 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_inspection_distinguishes_archive_and_extracted_tree_corruption() {
        let (root, cache, _) = registry_fixture("inspect-corrupt");
        resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap();
        fs::write(cache.join("demo-1.2.3/src/lib.fusion"), "tampered\n").unwrap();
        let reports = super::inspect_cache(&cache).unwrap();
        assert_eq!(reports[0].status, "tree-corrupt");
        fs::write(cache.join("demo-1.2.3.tar"), b"tampered archive").unwrap();
        let reports = super::inspect_cache(&cache).unwrap();
        assert_eq!(reports[0].status, "archive-corrupt");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cache_inspection_reports_malformed_metadata_without_aborting_scan() {
        let root = std::env::temp_dir().join(format!("foundry-registry-inspect-malformed-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("registry-meta-bad-1.0.0.toml"), "not = [valid").unwrap();
        let reports = super::inspect_cache(&root).unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].status, "metadata-invalid");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_registry_pins_do_not_require_a_version_index() {
        assert_eq!(exact_version_requirement("=1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(exact_version_requirement("1.2.3").as_deref(), Some("1.2.3"));
        assert_eq!(exact_version_requirement("=v1.2.3").as_deref(), Some("1.2.3"));
        assert!(exact_version_requirement("^1.2").is_none());
        assert!(exact_version_requirement("~1.2").is_none());
    }

    #[test]
    fn offline_exact_registry_pin_uses_cached_metadata_without_versions_index() {
        let (root, cache, _) = registry_fixture("exact");
        let resolved = resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap();
        assert_eq!(resolved.version, "1.2.3");
        assert_eq!(resolved.registry_url, root.to_string_lossy().to_string());
        assert!(!cache.join("registry-index-demo-versions.txt").exists());
        assert_eq!(fs::read_to_string(cache.join("demo-1.2.3/src/lib.fusion")).unwrap(), "pub const VALUE = 3\n");
        let tree_manifest = fs::read_to_string(cache.join("demo-1.2.3.tree.toml")).unwrap();
        let parsed_manifest: toml::Value = toml::from_str(&tree_manifest).unwrap();
        assert_eq!(parsed_manifest.get("format_version").and_then(toml::Value::as_integer), Some(1));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn modified_extracted_registry_cache_is_repaired_from_checksum_verified_archive() {
        let (root, cache, _) = registry_fixture("tamper");
        resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap();
        let extracted = cache.join("demo-1.2.3");
        fs::write(extracted.join("src/lib.fusion"), "pub const VALUE = 999\n").unwrap();
        fs::write(extracted.join("injected.txt"), "untrusted cache mutation").unwrap();

        resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap();
        assert_eq!(fs::read_to_string(extracted.join("src/lib.fusion")).unwrap(), "pub const VALUE = 3\n");
        assert!(!extracted.join("injected.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupted_tree_manifest_triggers_verified_reextraction() {
        let (root, cache, _) = registry_fixture("manifest");
        resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap();
        fs::write(cache.join("demo-1.2.3.tree.toml"), "format_version = 1\narchive_sha256 = \"wrong\"\ntree_sha256 = \"bad\"\n").unwrap();
        fs::write(cache.join("demo-1.2.3/src/lib.fusion"), "tampered\n").unwrap();

        resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap();
        assert_eq!(fs::read_to_string(cache.join("demo-1.2.3/src/lib.fusion")).unwrap(), "pub const VALUE = 3\n");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn archive_checksum_mismatch_is_rejected_without_changing_extracted_sources() {
        let (root, cache, _) = registry_fixture("archive-tamper");
        resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap();
        let archive_path = cache.join("demo-1.2.3.tar");
        fs::write(&archive_path, b"modified archive bytes").unwrap();

        let error = resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap_err();
        assert!(error.contains("checksum mismatch"));
        assert!(!archive_path.exists());
        assert_eq!(fs::read_to_string(cache.join("demo-1.2.3/src/lib.fusion")).unwrap(), "pub const VALUE = 3\n");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_archive_does_not_replace_existing_extracted_package() {
        let (root, cache, _) = registry_fixture("preserve");
        let extracted = cache.join("demo-1.2.3");
        fs::create_dir_all(&extracted).unwrap();
        fs::write(extracted.join("known-good.txt"), "keep me").unwrap();
        fs::write(cache.join("demo-1.2.3.tree.toml"), "invalid manifest").unwrap();
        fs::write(cache.join("demo-1.2.3.tar"), b"not a tar archive").unwrap();
        let checksum = hex::encode(Sha256::digest(b"not a tar archive"));
        fs::write(cache.join("registry-meta-demo-1.2.3.toml"), format!(
            "format_version = 1\nname = \"demo\"\nversion = \"1.2.3\"\narchive = \"/unused/archive.tar\"\nchecksum = \"{checksum}\"\n"
        )).unwrap();

        let error = resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap_err();
        assert!(error.contains("inspect package archive") || error.contains("could not inspect package archive"));
        assert_eq!(fs::read_to_string(extracted.join("known-good.txt")).unwrap(), "keep me");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_package_names_cannot_escape_the_cache_directory() {
        let root = std::env::temp_dir().join("foundry-registry-name-check");
        let error = resolve("../escape", "=1.2.3", "file:///unused", &root, true).unwrap_err();
        assert!(error.contains("invalid registry package name"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registry_archives_with_parent_traversal_are_rejected_before_extraction() {
        let (root, cache, _) = registry_fixture("traversal");
        let source = root.join("source");
        let archive_path = cache.join("demo-1.2.3.tar");
        let status = Command::new("tar").arg("--transform=s,^src,../escape,")
            .arg("-cf").arg(&archive_path).arg("-C").arg(&source).arg("src")
            .status().unwrap();
        assert!(status.success());
        let checksum = hex::encode(Sha256::digest(fs::read(&archive_path).unwrap()));
        fs::write(cache.join("registry-meta-demo-1.2.3.toml"), format!(
            "format_version = 1\nname = \"demo\"\nversion = \"1.2.3\"\narchive = \"/unused/archive.tar\"\nchecksum = \"{checksum}\"\n"
        )).unwrap();
        let error = resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap_err();
        assert!(error.contains("unsafe path"));
        assert!(!cache.join("demo-1.2.3").exists());
        assert!(!cache.join("escape").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn registry_archives_with_symlinks_are_rejected_before_extraction() {
        use std::os::unix::fs::symlink;
        let (root, cache, _) = registry_fixture("symlink");
        let source = root.join("source");
        symlink("lib.fusion", source.join("src/link.fusion")).unwrap();
        let archive_path = cache.join("demo-1.2.3.tar");
        let status = Command::new("tar").arg("-cf").arg(&archive_path)
            .arg("-C").arg(&source).arg("src").status().unwrap();
        assert!(status.success());
        let checksum = hex::encode(Sha256::digest(fs::read(&archive_path).unwrap()));
        fs::write(cache.join("registry-meta-demo-1.2.3.toml"), format!(
            "format_version = 1\nname = \"demo\"\nversion = \"1.2.3\"\narchive = \"/unused/archive.tar\"\nchecksum = \"{checksum}\"\n"
        )).unwrap();
        let error = resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, true).unwrap_err();
        assert!(error.contains("link or unsupported special entry"));
        assert!(!cache.join("demo-1.2.3").exists());
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn registry_metadata_requires_supported_schema_and_exact_identity() {
        let checksum = "a".repeat(64);
        let valid = format!("format_version = 1\nname = \"demo\"\nversion = \"1.2.3\"\narchive = \"/tmp/demo.tar\"\nchecksum = \"{checksum}\"\n");
        assert!(super::validate_registry_metadata(&valid, "demo", "1.2.3").is_ok());
        let missing_version = valid.replace("format_version = 1\n", "");
        assert!(super::validate_registry_metadata(&missing_version, "demo", "1.2.3").unwrap_err().contains("missing integer format_version"));
        let future_version = valid.replace("format_version = 1", "format_version = 2");
        assert!(super::validate_registry_metadata(&future_version, "demo", "1.2.3").unwrap_err().contains("unsupported registry metadata format_version 2"));
        let wrong_identity = valid.replace("name = \"demo\"", "name = \"other\"");
        assert!(super::validate_registry_metadata(&wrong_identity, "demo", "1.2.3").unwrap_err().contains("identity mismatch"));
    }

    #[test]
    fn registry_archive_locations_reject_insecure_or_ambiguous_schemes() {
        assert!(super::validate_archive_location("https://packages.example/demo.tar").is_ok());
        assert!(super::validate_archive_location("file:///tmp/demo.tar").is_ok());
        assert!(super::validate_archive_location("/tmp/demo.tar").is_ok());
        for invalid in ["http://packages.example/demo.tar", "ftp://packages.example/demo.tar", "https://user:pass@packages.example/demo.tar", "relative/demo.tar", "https://packages.example/demo.tar#fragment"] {
            assert!(super::validate_archive_location(invalid).is_err(), "accepted unsafe archive location {invalid}");
        }
    }

    #[test]
    fn registry_base_requires_https_or_absolute_local_directory() {
        assert!(super::validate_registry_base("https://registry.example").is_ok());
        assert!(super::validate_registry_base("/tmp/local-registry").is_ok());
        assert!(super::validate_registry_base("file:///tmp/local-registry").is_ok());
        for invalid in ["http://registry.example", "ftp://registry.example", "relative/registry", "https://user@registry.example", "https://registry.example?token=x"] {
            assert!(super::validate_registry_base(invalid).is_err(), "accepted invalid registry base {invalid}");
        }
    }

    #[test]
    fn local_registry_range_resolution_uses_version_index_and_versioned_metadata() {
        let (root, cache, _) = registry_fixture("local-range-integration");
        let versions = root.join("packages/demo/versions.txt");
        fs::write(&versions, "1.0.0\n1.2.3\n").unwrap();
        let resolved = resolve("demo", "^1.0.0", root.to_str().unwrap(), &cache, false).unwrap();
        assert_eq!(resolved.version, "1.2.3");
        assert_eq!(fs::read_to_string(cache.join("registry-index-demo-versions.txt")).unwrap(), "1.0.0\n1.2.3\n");
        assert!(fs::read_to_string(cache.join("registry-meta-demo-1.2.3.toml")).unwrap().contains("format_version = 1"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn online_registry_resolution_rejects_unversioned_metadata_before_cache_publication() {
        let (root, cache, _) = registry_fixture("legacy-metadata");
        let metadata = root.join("packages/demo/1.2.3.toml");
        let text = fs::read_to_string(&metadata).unwrap().replace("format_version = 1\n", "");
        fs::write(metadata, text).unwrap();
        let error = resolve("demo", "=1.2.3", root.to_str().unwrap(), &cache, false).unwrap_err();
        assert!(error.contains("missing integer format_version"));
        let cached = fs::read_to_string(cache.join("registry-meta-demo-1.2.3.toml")).unwrap();
        assert!(cached.contains("format_version = 1"), "failed metadata validation must not replace the prior cache record");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn compound_ranges_require_every_clause_to_match() {
        assert!(satisfies("1.8.2", ">=1.4.0, <2.0.0"));
        assert!(!satisfies("2.0.0", ">=1.4.0, <2.0.0"));
        assert!(!satisfies("1.3.9", ">=1.4.0, <2.0.0"));
    }

    #[test]
    fn workspace_constraints_choose_highest_common_version() {
        let constraints = vec![
            VersionConstraint { requester: "app".into(), requirement: "^1.2.0".into() },
            VersionConstraint { requester: "ui-kit".into(), requirement: ">=1.5.0, <2.0.0".into() },
        ];
        let versions = vec!["1.2.0".into(), "1.5.0".into(), "1.9.4".into(), "2.0.0".into()];
        assert_eq!(select_version_for_constraints("shared", &constraints, &versions).unwrap(), "1.9.4");
    }

    #[test]
    fn workspace_constraint_conflict_names_each_requester() {
        let constraints = vec![
            VersionConstraint { requester: "app".into(), requirement: "^1.0.0".into() },
            VersionConstraint { requester: "legacy-plugin".into(), requirement: "^2.0.0".into() },
        ];
        let versions = vec!["1.9.0".into(), "2.1.0".into()];
        let error = select_version_for_constraints("shared", &constraints, &versions).unwrap_err();
        assert!(error.contains("app requires ^1.0.0"));
        assert!(error.contains("legacy-plugin requires ^2.0.0"));
    }

}
