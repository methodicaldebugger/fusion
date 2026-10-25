//contents of lib.rs

use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const MANIFEST: &str = "Foundry.toml";
pub const LEGACY_MANIFEST: &str = "fusion.toml";
pub const LOCKFILE: &str = "foundry.lock";

pub mod fingerprint;
pub mod graph;
pub mod manifest;
pub mod plugin;
pub mod process;
pub mod template;
pub mod registry;
pub mod cache;
pub mod toolchain;
pub mod test_harness;

#[derive(Debug, Clone, Default)]
pub struct Manifest {
    pub package: Package,
    pub dependencies: BTreeMap<String, Dependency>,
    pub dev_dependencies: BTreeMap<String, Dependency>,
    pub plugins: BTreeMap<String, Dependency>,
    pub toolchain: Toolchain,
}
#[derive(Debug, Clone, Default)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub edition: String,
    pub description: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Dependency {
    Version(String),
    Path(PathBuf),
    Git { url: String, rev: Option<String> },
}
#[derive(Debug, Clone, Default)]
pub struct Toolchain {
    pub fusion: Option<String>,
    pub channel: Option<String>,
}
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
}
#[derive(Debug, Clone, Copy)]
pub enum BuildProfile {
    Debug,
    Release,
}
impl BuildProfile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }
}

pub fn project_root(start: &Path) -> io::Result<PathBuf> {
    let mut p = if start.is_file() {
        start.parent().unwrap_or(start).to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if p.join(MANIFEST).is_file() || p.join(LEGACY_MANIFEST).is_file() {
            return Ok(p);
        }
        if !p.pop() {
            break;
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "could not find fusion.toml in this directory or a parent directory",
    ))
}

pub fn load_project(root: &Path) -> Result<Project, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("project root: {e}"))?;
    let manifest_path = if root.join(MANIFEST).is_file() { root.join(MANIFEST) } else { root.join(LEGACY_MANIFEST) };
    let text = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    Ok(Project {
        root,
        manifest: parse_manifest(&text)?,
    })
}

pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    // Parse the manifest as TOML instead of splitting lines manually. This correctly
    // handles escaped strings, comments, dotted keys, and inline tables.
    let value: toml::Value = toml::from_str(text)
        .map_err(|e| format!("invalid Foundry manifest: {e}"))?;
    let root = value.as_table().ok_or("manifest root must be a TOML table")?;
    let package = root
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or("manifest: [package] table is required")?;

    let get_string = |table: &toml::map::Map<String, toml::Value>, key: &str| -> Result<Option<String>, String> {
        match table.get(key) {
            None => Ok(None),
            Some(v) => v.as_str().map(|s| Some(s.to_owned()))
                .ok_or_else(|| format!("manifest: '{key}' must be a string")),
        }
    };

    let name = get_string(package, "name")?
        .filter(|s| !s.trim().is_empty())
        .ok_or("manifest: [package].name is required and must not be empty")?;
    if !valid_manifest_name(&name) {
        return Err(format!("manifest: invalid package name '{name}' (use letters, digits, '_' or '-')"));
    }
    let mut m = Manifest::default();
    m.package.name = name;
    m.package.version = get_string(package, "version")?.unwrap_or_else(|| "0.1.0".into());
    m.package.edition = get_string(package, "edition")?.unwrap_or_else(|| "2021".into());
    m.package.description = get_string(package, "description")?;

    for section in ["dependencies", "dev-dependencies", "plugins"] {
        if let Some(table) = root.get(section) {
            let table = table.as_table().ok_or_else(|| format!("manifest: [{section}] must be a table"))?;
            for (dependency_name, spec) in table {
                if !valid_manifest_name(dependency_name) {
                    return Err(format!("manifest: invalid dependency name '{dependency_name}'"));
                }
                let parsed = parse_dependency_value(spec, dependency_name)?;
                match section {
                    "dependencies" => { m.dependencies.insert(dependency_name.clone(), parsed); }
                    "dev-dependencies" => { m.dev_dependencies.insert(dependency_name.clone(), parsed); }
                    "plugins" => { m.plugins.insert(dependency_name.clone(), parsed); }
                    _ => unreachable!(),
                }
            }
        }
    }

    if let Some(toolchain) = root.get("toolchain") {
        let table = toolchain.as_table().ok_or("manifest: [toolchain] must be a table")?;
        m.toolchain.fusion = get_string(table, "fusion")?;
        m.toolchain.channel = get_string(table, "channel")?;
    }

    // Fail clearly on unsupported top-level sections rather than silently ignoring them.
    for key in root.keys() {
        if !matches!(key.as_str(), "package" | "dependencies" | "dev-dependencies" | "plugins" | "toolchain") {
            return Err(format!("manifest: unsupported top-level section or key '{key}'"));
        }
    }
    Ok(m)
}

fn valid_manifest_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn parse_dependency_value(value: &toml::Value, name: &str) -> Result<Dependency, String> {
    if let Some(version) = value.as_str() {
        if version.trim().is_empty() {
            return Err(format!("manifest: dependency '{name}' has an empty version requirement"));
        }
        return Ok(Dependency::Version(version.to_owned()));
    }
    let table = value.as_table().ok_or_else(|| format!("manifest: invalid dependency specification for '{name}'"))?;
    let version = table.get("version").and_then(toml::Value::as_str).map(str::to_owned);
    let path = table.get("path").and_then(toml::Value::as_str).map(PathBuf::from);
    let git = table.get("git").and_then(toml::Value::as_str).map(str::to_owned);
    let source_count = usize::from(version.is_some()) + usize::from(path.is_some()) + usize::from(git.is_some());
    if source_count != 1 {
        return Err(format!("manifest: dependency '{name}' must specify exactly one of version, path, or git"));
    }
    let rev = table.get("rev").and_then(toml::Value::as_str).map(str::to_owned);
    let tag = table.get("tag").and_then(toml::Value::as_str);
    let branch = table.get("branch").and_then(toml::Value::as_str);
    if (rev.is_some() || tag.is_some() || branch.is_some()) && git.is_none() {
        return Err(format!("manifest: dependency '{name}' uses rev/tag/branch without git"));
    }
    if usize::from(rev.is_some()) + usize::from(tag.is_some()) + usize::from(branch.is_some()) > 1 {
        return Err(format!("manifest: dependency '{name}' must not specify more than one of rev, tag, or branch"));
    }
    if let Some(path) = path {
        Ok(Dependency::Path(path))
    } else if let Some(git) = git {
        let selector = rev.or_else(|| tag.map(str::to_owned)).or_else(|| branch.map(str::to_owned));
        Ok(Dependency::Git { url: git, rev: selector })
    } else {
        Ok(Dependency::Version(version.unwrap_or_default()))
    }
}

fn strip_comment(s: &str) -> &str {
    let mut q = false;
    let mut esc = false;
    for (i, c) in s.char_indices() {
        if c == '"' && !esc {
            q = !q;
        }
        if c == '#' && !q {
            return &s[..i];
        }
        esc = c == '\\' && !esc;
        if c != '\\' {
            esc = false;
        }
    }
    s
}
fn parse_string(v: &str) -> Result<String, String> {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Ok(v[1..v.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"))
    } else {
        Err(format!("expected quoted string, got {v}"))
    }
}
fn parse_dependency(v: &str) -> Result<Dependency, String> {
    let v = v.trim();
    if v.starts_with('"') {
        return Ok(Dependency::Version(parse_string(v)?));
    }
    if v.starts_with('{') && v.ends_with('}') {
        let mut version = None;
        let mut path = None;
        let mut git = None;
        let mut rev = None;
        for part in v[1..v.len() - 1].split(',') {
            if let Some((k, val)) = part.split_once('=') {
                match k.trim() {
                    "version" => version = Some(parse_string(val)?),
                    "path" => path = Some(PathBuf::from(parse_string(val)?)),
                    "git" => git = Some(parse_string(val)?),
                    "rev" => rev = Some(parse_string(val)?),
                    _ => {}
                }
            }
        }
        if let Some(p) = path {
            return Ok(Dependency::Path(p));
        }
        if let Some(u) = git {
            return Ok(Dependency::Git { url: u, rev });
        }
        if let Some(v) = version {
            return Ok(Dependency::Version(v));
        }
    }
    Err(format!("invalid dependency specification: {v}"))
}

pub fn manifest_template(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n"
    )
}

fn directory_revision(root: &Path) -> String {
    fn walk(dir:&Path, out:&mut Vec<PathBuf>) {
        let Ok(entries)=fs::read_dir(dir) else{return};
        for entry in entries.flatten(){let path=entry.path();
            if path.file_name().map(|n|n==".git"||n=="target").unwrap_or(false){continue;}
            if path.is_dir(){walk(&path,out)}else if path.is_file(){out.push(path)}
        }
    }
    let mut files=Vec::new();walk(root,&mut files);files.sort();
    let mut fields=Vec::<Vec<u8>>::new();
    for file in files { fields.push(file.strip_prefix(root).unwrap_or(&file).to_string_lossy().as_bytes().to_vec()); if let Ok(bytes)=fs::read(&file){fields.push(bytes)} }
    fingerprint::fingerprint_fields(fields.iter().map(|v|v.as_slice()))
}

pub fn write_lock(project: &Project) -> Result<(), String> {
    let manifest_path = if project.root.join(MANIFEST).is_file() { project.root.join(MANIFEST) } else { project.root.join(LEGACY_MANIFEST) };
    let manifest_bytes = fs::read(&manifest_path).map_err(|e| format!("read manifest for lockfile: {e}"))?;
    let manifest_hash = fingerprint::fingerprint_fields([manifest_bytes.as_slice()]);
    let mut out = format!("# This file is generated by Foundry. Do not edit by hand.\nversion = 2\nmanifest-sha256 = {:?}\n\n", manifest_hash);
    for (name, dep) in project.manifest.dependencies.iter().chain(project.manifest.dev_dependencies.iter()).chain(project.manifest.plugins.iter()) {
        let (requested, source, revision, checksum) = match dep {
            Dependency::Version(v) => (v.clone(), "registry".to_string(), String::new(), String::new()),
            Dependency::Path(p) => {
                let full = project.root.join(p);
                let canonical = full.canonicalize().unwrap_or(full);
                let rev = if canonical.exists(){directory_revision(&canonical)}else{"missing-path".into()};
                ("path".into(), format!("path+{}", canonical.display()), rev, String::new())
            },
            Dependency::Git { url, rev } => {
                let dst = project.root.join("target/foundry/deps").join(name);
                let actual = if dst.join(".git").exists() { Command::new("git").current_dir(&dst).args(["rev-parse", "HEAD"]).output().ok().filter(|o|o.status.success()).map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(||rev.clone().unwrap_or_else(||"unresolved".into())) } else { rev.clone().unwrap_or_else(||"unresolved".into()) };
                (rev.clone().unwrap_or_else(||"*".into()), format!("git+{url}"), actual, String::new())
            }
        };
        let lock_version = if matches!(dep, Dependency::Version(_)) { "unresolved" } else { requested.as_str() };
        out.push_str(&format!("[[package]]\nname = {:?}\nrequested = {:?}\nversion = {:?}\nsource = {:?}\nrevision = {:?}\nchecksum = {:?}\n\n", name, requested, lock_version, source, revision, checksum));
    }
    fs::write(project.root.join(LOCKFILE), out).map_err(|e| format!("write lockfile: {e}"))
}

fn write_resolved_lock(project: &Project, resolved: &[registry::ResolvedPackage]) -> Result<(), String> {
    let manifest_path = if project.root.join(MANIFEST).is_file() { project.root.join(MANIFEST) } else { project.root.join(LEGACY_MANIFEST) };
    let bytes = fs::read(&manifest_path).map_err(|e| format!("read manifest for lockfile: {e}"))?;
    let hash = fingerprint::fingerprint_fields([bytes.as_slice()]);
    let mut out = format!("# This file is generated by Foundry. Do not edit by hand.\nversion = 2\nmanifest-sha256 = {:?}\n\n", hash);
    for r in resolved { out.push_str(&format!("[[package]]\nname = {:?}\nrequested = {:?}\nversion = {:?}\nsource = {:?}\nrevision = {:?}\nchecksum = {:?}\n\n",r.name,r.requested,r.version,r.source,"",r.checksum.as_deref().unwrap_or(""))); }
    // Preserve deterministic records for path and Git dependencies alongside registry resolutions.
    for (name, dep) in project.manifest.dependencies.iter().chain(project.manifest.dev_dependencies.iter()).chain(project.manifest.plugins.iter()) {
        if resolved.iter().any(|r|r.name==*name) { continue; }
        let (requested,source,revision)=match dep {
            Dependency::Version(v)=>(v.clone(),"registry".to_string(),"unresolved".to_string()),
            Dependency::Path(path)=>{let full=project.root.join(path).canonicalize().unwrap_or(project.root.join(path));let rev=if full.exists(){directory_revision(&full)}else{"missing-path".into()};("path".into(),format!("path+{}",full.display()),rev)},
            Dependency::Git{url,rev}=>{let dst=project.root.join("target/foundry/deps").join(name);let actual=if dst.join(".git").exists(){Command::new("git").current_dir(&dst).args(["rev-parse","HEAD"]).output().ok().filter(|o|o.status.success()).map(|o|String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(||rev.clone().unwrap_or_else(||"unresolved".into()))}else{rev.clone().unwrap_or_else(||"unresolved".into())};(rev.clone().unwrap_or_else(||"*".into()),format!("git+{url}"),actual)}
        };
        out.push_str(&format!("[[package]]\nname = {:?}\nrequested = {:?}\nversion = {:?}\nsource = {:?}\nrevision = {:?}\nchecksum = {:?}\n\n",name,requested,requested,source,revision,String::new()));
    }
    fs::write(project.root.join(LOCKFILE),out).map_err(|e|format!("write lockfile: {e}"))
}

pub fn ensure_layout(project: &Project) -> Result<(), String> {
    for d in ["src", "tests", "foreign", "target"] {
        fs::create_dir_all(project.root.join(d)).map_err(|e| format!("create {d}: {e}"))?;
    }
    let main = project.root.join("src/main.fusion");
    if !main.exists() {
        fs::write(main, "main:\n    print(\"Hello from Fusion!\")\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn resolve_dependencies(project: &Project, offline: bool) -> Result<(), String> {
    resolve_dependencies_with_update(project, offline, false)
}

pub fn resolve_dependencies_with_update(project: &Project, offline: bool, update: bool) -> Result<(), String> {
    let cache = project.root.join("target/foundry/deps");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let registry_url = env::var("FOUNDRY_REGISTRY").ok();
    let manifest_path = if project.root.join(MANIFEST).is_file() { project.root.join(MANIFEST) } else { project.root.join(LEGACY_MANIFEST) };
    let manifest_bytes = fs::read(&manifest_path).map_err(|e|format!("read manifest: {e}"))?;
    let manifest_hash = fingerprint::fingerprint_fields([manifest_bytes.as_slice()]);
    let lock_path=project.root.join(LOCKFILE);
    
    let locked = if !update && lock_path.is_file() {
        fs::read_to_string(&lock_path)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .filter(|v| {
                v.get("manifest-sha256")
                    .and_then(toml::Value::as_str)
                    == Some(manifest_hash.as_str())
            })
            .map(|v| {
                let mut map = BTreeMap::new();

                if let Some(pkgs) = v.get("package").and_then(toml::Value::as_array) {
                    for p in pkgs {
                        if let (Some(n), Some(ver)) = (
                            p.get("name").and_then(toml::Value::as_str),
                            p.get("version").and_then(toml::Value::as_str),
                        ) {
                            if registry::is_exact_version(ver) {
                                let checksum = p
                                    .get("checksum")
                                    .and_then(toml::Value::as_str)
                                    .unwrap_or("")
                                    .to_string();

                                map.insert(n.to_string(), (ver.to_string(), checksum));
                            }
                        }
                    }
                }

                map
            })
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };

    let mut resolved = Vec::new();
    for (name, dep) in project.manifest.dependencies.iter().chain(project.manifest.dev_dependencies.iter()).chain(project.manifest.plugins.iter()) 
    {
        match dep {
            Dependency::Path(p) => { let full=project.root.join(p); if !full.exists(){return Err(format!("dependency '{name}' path does not exist: {}",full.display()));} },
            Dependency::Git { url, rev } => {
                let dst=cache.join(name);
                if !dst.exists() { if offline { return Err(format!("Git dependency '{name}' is not cached (--offline)")); } let mut c=Command::new("git");c.args(["clone",url]).arg(&dst);if !run(&mut c,&project.root)?{return Err(format!("failed to clone dependency '{name}'"));} }
                else if !offline { let mut c=Command::new("git");c.current_dir(&dst).args(["fetch","--all"]);if !run(&mut c,&dst)?{return Err(format!("failed to update dependency '{name}'"));} }
                if let Some(r)=rev { let mut c=Command::new("git");c.current_dir(&dst).args(["checkout",r]);if !run(&mut c,&dst)?{return Err(format!("failed to checkout {r} for '{name}'"));} }
            },
            Dependency::Version(req) => {
                let registry=registry_url.as_deref().ok_or_else(||format!("dependency '{name}' requires a registry version '{req}', but FOUNDRY_REGISTRY is not set (expected a registry URL or local registry path)"))?;
                let requirement=if !update { locked.get(name).map(|(v,_)|format!("={v}")).unwrap_or_else(||req.clone()) } else { req.clone() };
                let package=registry::resolve(name,&requirement,registry,&cache,offline)?;
                if !registry::satisfies(&package.version,req) { return Err(format!("locked version {} of '{name}' no longer satisfies manifest requirement '{req}'",package.version)); }
                if let Some((_,checksum))=locked.get(name) { if !checksum.is_empty() && package.checksum.as_deref()!=Some(checksum.as_str()) { return Err(format!("registry checksum changed for locked dependency '{name}' {}; run 'foundry update' only if you intend to refresh it",package.version)); } }
                resolved.push(registry::ResolvedPackage{requested:req.clone(),..package});
            }
        }
    }
    write_resolved_lock(project,&resolved)?;
    Ok(())
}

fn run(c: &mut Command, cwd: &Path) -> Result<bool, String> {
    c.current_dir(cwd);
    Ok(c.status()
        .map_err(|e| format!("could not start command: {e}"))?
        .success())
}
fn find_on_path(name: &str) -> Option<PathBuf> {
    let p = env::var_os("PATH")?;
    for d in env::split_paths(&p) {
        let x = d.join(name);
        if x.is_file() {
            return Some(x);
        }
    }
    None
}
pub fn find_fusion(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        return Ok(p.canonicalize().unwrap_or_else(|_| p.to_path_buf()));
    }
    if let Ok(p) = env::var("FUSION_COMPILER") {
        let path = PathBuf::from(p);
        return Ok(path.canonicalize().unwrap_or(path));
    }
    let name = if cfg!(windows) {
        "fusion.exe"
    } else {
        "fusion"
    };
    if let Some(p) = find_on_path(name) {
        return Ok(p);
    }
    if let Ok(me) = env::current_exe() {
        for base in me.ancestors().take(5) {
            for p in [
                base.join("fusion"),
                base.join("target/debug/fusion"),
                base.join("target/release/fusion"),
            ] {
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
    }
    Err("could not find Fusion compiler; set FUSION_COMPILER or put fusion on PATH".into())
}
/// Resolve a project-pinned compiler. A toolchain feed entry is TOML with `version`, `url`,
/// `sha256`, and `executable`; the archive is verified before it is used.
pub fn find_project_fusion(project: &Project, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if explicit.is_some() || env::var_os("FUSION_COMPILER").is_some() || project.manifest.toolchain.fusion.is_none() {
        return find_fusion(explicit);
    }
    let version=project.manifest.toolchain.fusion.as_deref().unwrap();
    let feed=env::var("FOUNDRY_TOOLCHAIN_FEED").map_err(|_|format!("project pins Fusion {version}, but FOUNDRY_TOOLCHAIN_FEED is not set"))?;
    let url=format!("{}/{}.toml",feed.trim_end_matches('/'),version);
    let metadata=if let Some(path)=url.strip_prefix("file://") { fs::read_to_string(path).map_err(|e|e.to_string())? } else if !url.contains("://") { fs::read_to_string(&url).map_err(|e|e.to_string())? } else {
        let out=Command::new("curl").args(["--fail","--silent","--show-error","--location"]).arg(&url).output().map_err(|e|format!("start curl for toolchain metadata: {e}"))?;
        if !out.status.success(){return Err(format!("could not retrieve pinned toolchain metadata: {url}"));} String::from_utf8(out.stdout).map_err(|e|e.to_string())?
    };
    let value:toml::Value=toml::from_str(&metadata).map_err(|e|format!("invalid toolchain metadata: {e}"))?;
    let actual=value.get("version").and_then(toml::Value::as_str).unwrap_or("");
    if actual!=version{return Err(format!("toolchain feed returned version '{actual}', but project pins '{version}'"));}
    let url=value.get("url").and_then(toml::Value::as_str).ok_or("toolchain metadata missing url")?.to_string();
    let sha256=value.get("sha256").and_then(toml::Value::as_str).ok_or("toolchain metadata missing sha256")?.to_string();
    let executable=value.get("executable").and_then(toml::Value::as_str).unwrap_or(if cfg!(windows){"fusion.exe"}else{"fusion"}).to_string();
    let home=env::var_os("FOUNDRY_HOME").map(PathBuf::from).or_else(||env::var_os("HOME").map(|h|PathBuf::from(h).join(".foundry"))).unwrap_or_else(||project.root.join("target/foundry-home"));
    let path=toolchain::install(&toolchain::ToolchainSpec{version:version.into(),url,sha256,executable},&home.join("toolchains"),env::var_os("FOUNDRY_OFFLINE").is_some())?;
    Ok(path)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out)
        } else if p.extension() == Some(OsStr::new("fusion")) {
            out.push(p)
        }
    }
}
pub fn source_files(p: &Project) -> Vec<PathBuf> {
    let mut v = Vec::new();
    collect(&p.root.join("src"), &mut v);
    v.sort();
    v
}
pub fn test_files(p: &Project) -> Vec<PathBuf> {
    let mut v = Vec::new();
    collect(&p.root.join("tests"), &mut v);
    v.sort();
    v
}
pub fn main_source(p: &Project) -> Result<PathBuf, String> {
    let x = p.root.join("src/main.fusion");
    if x.is_file() {
        Ok(x)
    } else {
        source_files(p)
            .into_iter()
            .next()
            .ok_or_else(|| "no .fusion source files found in src/".into())
    }
}

pub fn build(p: &Project, profile: BuildProfile, fusion: Option<&Path>, extra: &[String]) -> Result<PathBuf, String> {
    build_inner(p, profile, fusion, extra, true)
}

fn build_inner(p: &Project, profile: BuildProfile, fusion: Option<&Path>, extra: &[String], resolve: bool) -> Result<PathBuf, String> {
    if resolve {
        resolve_dependencies(p, env::var_os("FOUNDRY_OFFLINE").is_some())?;
    }
    let src = main_source(p)?;
    let target = extra.windows(2).find(|w|w[0]=="--target").map(|w|w[1].clone());
    let mut dir = p.root.join("target").join(profile.name());
    if let Some(t)=&target { dir=dir.join(t); }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join(if cfg!(windows) {
        format!("{}.exe", p.manifest.package.name)
    } else {
        p.manifest.package.name.clone()
    });
    let compiler = find_project_fusion(p, fusion)?;
    let mut fingerprint_inputs: Vec<Vec<u8>> = Vec::new();
    fingerprint_inputs.push(p.manifest.package.name.as_bytes().to_vec());
    fingerprint_inputs.push(p.manifest.package.version.as_bytes().to_vec());
    fingerprint_inputs.push(profile.name().as_bytes().to_vec());
    fingerprint_inputs.push(compiler.to_string_lossy().as_bytes().to_vec());
    fingerprint_inputs.push(format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH).into_bytes());
    if let Ok(version) = Command::new(&compiler).arg("--version").output() {
        fingerprint_inputs.push(version.stdout);
        fingerprint_inputs.push(version.stderr);
    }
    let manifest_path = if p.root.join(MANIFEST).is_file() { p.root.join(MANIFEST) } else { p.root.join(LEGACY_MANIFEST) };
    fingerprint_inputs.push(fs::read(&manifest_path).map_err(|e| format!("read manifest for fingerprint: {e}"))?);
    fingerprint_inputs.push(format!("{:?}", p.manifest.dependencies).into_bytes());
    if let Ok(lock_bytes)=fs::read(p.root.join(LOCKFILE)) { fingerprint_inputs.push(lock_bytes); }
    for dependency in p.manifest.dependencies.values().chain(p.manifest.dev_dependencies.values()) {
        match dependency {
            Dependency::Path(path)=>{let full=p.root.join(path);fingerprint_inputs.push(directory_revision(&full).into_bytes());},
            Dependency::Git{url,rev}=>{fingerprint_inputs.push(url.as_bytes().to_vec());fingerprint_inputs.push(rev.clone().unwrap_or_default().into_bytes());},
            Dependency::Version(version)=>fingerprint_inputs.push(version.as_bytes().to_vec()),
        }
    }
    fingerprint_inputs.extend(extra.iter().map(|x| x.as_bytes().to_vec()));
    for file in source_files(p) {
        fingerprint_inputs.push(file.strip_prefix(&p.root).unwrap_or(&file).to_string_lossy().as_bytes().to_vec());
        fingerprint_inputs.push(fs::read(&file).map_err(|e| format!("read {}: {e}", file.display()))?);
    }
    let refs = fingerprint_inputs.iter().map(|x| x.as_slice()).collect::<Vec<_>>();
    let fingerprint = fingerprint::fingerprint_fields(refs);
    let stamp = dir.join(".foundry-fingerprint");
    if out.is_file() && fs::read_to_string(&stamp).ok().as_deref() == Some(fingerprint.as_str()) {
        println!("Fresh {} (cached)", out.display()); return Ok(out);
    }
    if let Ok(remote)=env::var("FOUNDRY_REMOTE_CACHE") {
        if cache::restore(&remote,&fingerprint,&out)? { fs::write(&stamp,&fingerprint).map_err(|e|e.to_string())?; println!("Restored {} from remote cache",out.display()); return Ok(out); }
    }
    let mut c = Command::new(&compiler);
    c.current_dir(&p.root)
        .args(["build"])
        .arg(&src)
        .args(["-o"])
        .arg(&out)
        .args(extra);
    if !run(&mut c, &p.root)? {
        return Err("Fusion compilation failed".into());
    }
    fs::write(&stamp, &fingerprint).map_err(|e| format!("write build fingerprint: {e}"))?;
    if let Ok(remote)=env::var("FOUNDRY_REMOTE_CACHE") { if let Err(e)=cache::publish(&remote,&fingerprint,&out) { eprintln!("warning: {e}"); } }
    Ok(out)
}
/// Build several explicit targets concurrently. Each target receives an isolated output directory;
/// the dependency graph validates the independent target tasks before worker threads start.
pub fn build_targets(p: &Project, profile: BuildProfile, fusion: Option<&Path>, targets: &[String], extra: &[String]) -> Result<Vec<PathBuf>, String> {
    if targets.is_empty() { return Ok(vec![build(p,profile,fusion,extra)?]); }
    let mut graph=graph::TaskGraph::default();
    for target in targets { graph.add_task(format!("target:{target}"),std::iter::empty::<String>()).map_err(|e|e.to_string())?; }
    graph.execution_order().map_err(|e|e.to_string())?;
    resolve_dependencies(p, env::var_os("FOUNDRY_OFFLINE").is_some())?;
    let pinned_compiler=find_project_fusion(p,fusion)?;
    let results=std::thread::scope(|scope| {
        let handles=targets.iter().map(|target| {
            let project=p.clone(); let target=target.clone(); let compiler=pinned_compiler.clone(); let mut args=extra.to_vec();
            args.push("--target".into()); args.push(target.clone());
            scope.spawn(move || build_inner(&project,profile,Some(&compiler),&args,false).map(|path|(target,path)))
        }).collect::<Vec<_>>();
        handles.into_iter().map(|h|h.join().unwrap_or_else(|_|Err("target build worker panicked".into()))).collect::<Vec<_>>()
    });
    let mut outputs=Vec::new(); let mut errors=Vec::new();
    for result in results { match result {Ok((target,path))=>{println!("Target {target}: {}",path.display());outputs.push(path)},Err(e)=>errors.push(e)} }
    if errors.is_empty(){Ok(outputs)}else{Err(errors.join("; "))}
}

pub fn check(p: &Project, fusion: Option<&Path>) -> Result<(), String> {
    let compiler=find_project_fusion(p, fusion)?; let files=source_files(p);
    if files.is_empty(){return Err("no .fusion source files found in src/".into());}
    let root=&p.root;
    let failures=std::thread::scope(|scope| {
        let handles=files.iter().map(|src| { let compiler=compiler.clone(); let src=src.clone(); let root=root.clone(); scope.spawn(move || { let mut c=Command::new(compiler);c.current_dir(&root).args(["check"]).arg(&src);match c.status(){Ok(s) if s.success()=>None,Ok(_)=>Some(format!("check failed for {}",src.display())),Err(e)=>Some(format!("could not start compiler for {}: {e}",src.display()))} }) }).collect::<Vec<_>>();
        handles.into_iter().filter_map(|h|h.join().unwrap_or_else(|_|Some("check worker panicked".into()))).collect::<Vec<_>>()
    });
    if failures.is_empty(){Ok(())}else{Err(failures.join("; "))}
}

pub fn run_program(p: &Project, fusion: Option<&Path>, args: &[String]) -> Result<(), String> {
    let compiler = find_project_fusion(p, fusion)?;
    let mut c = Command::new(compiler);
    c.current_dir(&p.root)
        .args(["run"])
        .arg(main_source(p)?)
        .args(args);
    if !run(&mut c, &p.root)? {
        return Err("Fusion program exited with failure".into());
    }
    Ok(())
}
pub fn test(p: &Project, fusion: Option<&Path>) -> Result<(), String> {
    let compiler=find_project_fusion(p, fusion)?; let files=test_files(p);
    if files.is_empty(){println!("Foundry: no integration tests found in tests/");return Ok(());}
    test_harness::run_files(&compiler,&p.root,&files).map(|_|())
}

pub fn clean(p: &Project) -> Result<(), String> {
    // Keep downloaded dependencies and toolchains cached under target/foundry.
    // `clean` should remove generated build outputs, not force users to download
    // every dependency again.
    let target = p.root.join("target");
    for relative in ["debug", "release", "doc"] {
        let path = target.join(relative);
        if path.exists() {
            fs::remove_dir_all(&path)
                .map_err(|e| format!("remove build output {}: {e}", path.display()))?;
        }
    }
    Ok(())
}
pub fn tree(p: &Project) -> String {
    let mut s = format!(
        "{} v{}\n",
        p.manifest.package.name, p.manifest.package.version
    );
    for (n, d) in &p.manifest.dependencies {
        s.push_str(&format!("├── {n} {}\n", label(d)))
    }
    for (n, d) in &p.manifest.dev_dependencies {
        s.push_str(&format!("[dev]── {n} {}\n", label(d)))
    }
    s
}
fn label(d: &Dependency) -> String {
    match d {
        Dependency::Version(v) => v.clone(),
        Dependency::Path(p) => format!("path:{}", p.display()),
        Dependency::Git { url, rev } => format!(
            "git:{url}{}",
            rev.as_ref().map(|x| format!("#{x}")).unwrap_or_default()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semver_ranges_are_resolved_conservatively() {
        assert!(registry::satisfies("1.4.2", "^1.2"));
        assert!(!registry::satisfies("2.0.0", "^1.2"));
        assert!(registry::satisfies("1.2.9", "~1.2"));
        assert!(!registry::satisfies("1.3.0", "~1.2"));
        assert!(registry::satisfies("1.2.0", ">=1.2"));
    }

    #[test]
    fn manifest_parser_uses_toml_semantics_for_inline_tables_and_escapes() {
        let m = parse_manifest(r#"
[package]
name = "demo"
description = "line one # not a comment"

[dependencies]
path_dep = { path = "../libs/a,b" }
git_dep = { git = "https://example.invalid/repo.git", tag = "v1.2.0" }
"#).unwrap();
        assert_eq!(m.package.description.as_deref(), Some("line one # not a comment"));
        assert!(matches!(m.dependencies["path_dep"], Dependency::Path(_)));
        assert!(matches!(&m.dependencies["git_dep"], Dependency::Git { rev: Some(tag), .. } if tag == "v1.2.0"));
    }

    #[test]
    fn manifest_rejects_multiple_dependency_sources_and_unsafe_names() {
        let multiple = "[package]\\nname = \\"demo\\"\\n[dependencies]\\nx = { path = \\"../x\\", version = \\"1\\" }\\n";
        assert!(parse_manifest(multiple).is_err());
        let unsafe_name = "[package]\\nname = \\"demo\\"\\n[dependencies]\\n\\"../escape\\" = \\"1\\"\\n";
        assert!(parse_manifest(unsafe_name).is_err());
    }

    #[test]
    fn clean_preserves_dependency_cache() {
        let root = std::env::temp_dir().join(format!("foundry-clean-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("target/debug")).unwrap();
        fs::create_dir_all(root.join("target/foundry/deps")).unwrap();
        fs::write(root.join("target/debug/app"), "artifact").unwrap();
        fs::write(root.join("target/foundry/deps/keep"), "cached").unwrap();
        let project = Project { root: root.clone(), manifest: Manifest::default() };
        clean(&project).unwrap();
        assert!(!root.join("target/debug").exists());
        assert!(root.join("target/foundry/deps/keep").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_rejects_unsafe_package_name() {
        let input = r#"[package]
name = "../escape"
"#;
        assert!(parse_manifest(input).is_err());
    }

    #[test]
    fn manifest_parses() {
        let m=parse_manifest("[package]\nname=\"x\"\n[dependencies]\na=\"1\"\nb={ path=\"../b\" }\nc={ git=\"https://x/c.git\", rev=\"abc\" }\n").unwrap();
        assert_eq!(m.package.name, "x");
        assert!(matches!(m.dependencies["a"], Dependency::Version(_)));
        assert!(matches!(m.dependencies["b"], Dependency::Path(_)));
        assert!(matches!(m.dependencies["c"], Dependency::Git { .. }));
    }
}
