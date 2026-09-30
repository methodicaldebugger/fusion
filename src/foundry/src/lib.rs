use std::{collections::BTreeMap, env, ffi::OsStr, fs, io::{self, Write}, path::{Path, PathBuf}, process::{Command, Stdio}};

pub const MANIFEST: &str = "fusion.toml";
pub const LOCKFILE: &str = "foundry.lock";

#[derive(Debug, Clone, Default)]
pub struct Manifest { pub package: Package, pub dependencies: BTreeMap<String, Dependency>, pub dev_dependencies: BTreeMap<String, Dependency>, pub plugins: BTreeMap<String, Dependency>, pub toolchain: Toolchain }
#[derive(Debug, Clone, Default)]
pub struct Package { pub name: String, pub version: String, pub edition: String, pub description: Option<String> }
#[derive(Debug, Clone)]
pub enum Dependency { Version(String), Path(PathBuf), Git { url: String, rev: Option<String> } }
#[derive(Debug, Clone, Default)]
pub struct Toolchain { pub fusion: Option<String>, pub channel: Option<String> }
#[derive(Debug, Clone)]
pub struct Project { pub root: PathBuf, pub manifest: Manifest }
#[derive(Debug, Clone, Copy)]
pub enum BuildProfile { Debug, Release }
impl BuildProfile { pub fn name(self) -> &'static str { match self { Self::Debug => "debug", Self::Release => "release" } } }

pub fn project_root(start: &Path) -> io::Result<PathBuf> {
    let mut p = if start.is_file() { start.parent().unwrap_or(start).to_path_buf() } else { start.to_path_buf() };
    loop { if p.join(MANIFEST).is_file() { return Ok(p); } if !p.pop() { break; } }
    Err(io::Error::new(io::ErrorKind::NotFound, "could not find fusion.toml in this directory or a parent directory"))
}

pub fn load_project(root: &Path) -> Result<Project, String> {
    let root = root.canonicalize().map_err(|e| format!("project root: {e}"))?;
    let text = fs::read_to_string(root.join(MANIFEST)).map_err(|e| format!("read {}: {e}", root.join(MANIFEST).display()))?;
    Ok(Project { root, manifest: parse_manifest(&text)? })
}

pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    let mut m = Manifest::default(); let mut section = String::new();
    for (n, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim(); if line.is_empty() { continue; }
        if line.starts_with('[') && line.ends_with(']') { section = line[1..line.len()-1].trim().to_string(); continue; }
        let (k, v) = line.split_once('=').ok_or_else(|| format!("manifest:{}: expected key = value", n + 1))?;
        let k = k.trim(); let v = v.trim();
        match section.as_str() {
            "package" => match k { "name" => m.package.name=parse_string(v)?, "version"=>m.package.version=parse_string(v)?, "edition"=>m.package.edition=parse_string(v)?, "description"=>m.package.description=Some(parse_string(v)?), _=>{} },
            "dependencies" => { m.dependencies.insert(k.into(), parse_dependency(v)?); }
            "dev-dependencies" => { m.dev_dependencies.insert(k.into(), parse_dependency(v)?); }
            "plugins" => { m.plugins.insert(k.into(), parse_dependency(v)?); }
            "toolchain" => match k { "fusion"=>m.toolchain.fusion=Some(parse_string(v)?), "channel"=>m.toolchain.channel=Some(parse_string(v)?), _=>{} },
            _ => return Err(format!("manifest:{}: unknown or unsupported section [{}]", n + 1, section)),
        }
    }
    if m.package.name.is_empty() { return Err("manifest: [package].name is required".into()); }
    if m.package.version.is_empty() { m.package.version="0.1.0".into(); }
    if m.package.edition.is_empty() { m.package.edition="2021".into(); }
    Ok(m)
}

fn strip_comment(s: &str) -> &str { let mut q=false; let mut esc=false; for (i,c) in s.char_indices() { if c=='"' && !esc { q=!q; } if c=='#' && !q { return &s[..i]; } esc=c=='\\' && !esc; if c!='\\' { esc=false; } } s }
fn parse_string(v: &str) -> Result<String,String> { let v=v.trim(); if v.len()>=2 && v.starts_with('"') && v.ends_with('"') { Ok(v[1..v.len()-1].replace("\\\"","\"").replace("\\\\","\\")) } else { Err(format!("expected quoted string, got {v}")) } }
fn parse_dependency(v: &str) -> Result<Dependency,String> {
    let v=v.trim(); if v.starts_with('"') { return Ok(Dependency::Version(parse_string(v)?)); }
    if v.starts_with('{') && v.ends_with('}') { let mut version=None; let mut path=None; let mut git=None; let mut rev=None;
        for part in v[1..v.len()-1].split(',') { if let Some((k,val))=part.split_once('=') { match k.trim() { "version"=>version=Some(parse_string(val)?), "path"=>path=Some(PathBuf::from(parse_string(val)?)), "git"=>git=Some(parse_string(val)?), "rev"=>rev=Some(parse_string(val)?), _=>{} } } }
        if let Some(p)=path { return Ok(Dependency::Path(p)); } if let Some(u)=git { return Ok(Dependency::Git{url:u,rev}); } if let Some(v)=version { return Ok(Dependency::Version(v)); }
    }
    Err(format!("invalid dependency specification: {v}"))
}

pub fn manifest_template(name: &str) -> String { format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n") }

pub fn write_lock(project: &Project) -> Result<(),String> {
    let mut out="# This file is generated by Foundry.\nversion = 1\n\n".to_string();
    for (name,dep) in project.manifest.dependencies.iter().chain(project.manifest.dev_dependencies.iter()).chain(project.manifest.plugins.iter()) {
        let (version,source)=match dep { Dependency::Version(v)=>(v.clone(),"registry".into()), Dependency::Path(p)=>("path".into(),format!("path+{}",p.display())), Dependency::Git{url,rev}=>(rev.clone().unwrap_or_else(||"HEAD".into()),format!("git+{url}")) };
        out.push_str(&format!("[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = \"{source}\"\n\n"));
    }
    fs::write(project.root.join(LOCKFILE),out).map_err(|e|format!("write lockfile: {e}"))
}

pub fn ensure_layout(project:&Project)->Result<(),String>{ for d in ["src","tests","foreign","target"] { fs::create_dir_all(project.root.join(d)).map_err(|e|format!("create {d}: {e}"))?; } let main=project.root.join("src/main.fusion"); if !main.exists(){fs::write(main,"main:\n    print(\"Hello from Fusion!\")\n").map_err(|e|e.to_string())?;} Ok(()) }

pub fn resolve_dependencies(project:&Project,offline:bool)->Result<(),String>{
    write_lock(project)?; let cache=project.root.join("target/foundry/deps"); fs::create_dir_all(&cache).map_err(|e|e.to_string())?;
    for (name,dep) in &project.manifest.dependencies { match dep {
        Dependency::Path(p)=>{let full=project.root.join(p); if !full.exists(){return Err(format!("dependency '{name}' path does not exist: {}",full.display()));}},
        Dependency::Git{url,rev}=>{if offline{continue;} let dst=cache.join(name); if !dst.exists(){let mut c=Command::new("git");c.args(["clone",url]).arg(&dst);if !run(&mut c,&project.root)?{return Err(format!("failed to clone dependency '{name}'"));}} else {let mut c=Command::new("git");c.current_dir(&dst).args(["fetch","--all"]);if !run(&mut c,&dst)?{return Err(format!("failed to update dependency '{name}'"));}} if let Some(r)=rev{let mut c=Command::new("git");c.current_dir(&dst).args(["checkout",r]);if !run(&mut c,&dst)?{return Err(format!("failed to checkout {r} for '{name}'"));}}},
        Dependency::Version(_)=>{}
    }} Ok(())
}

fn run(c:&mut Command,cwd:&Path)->Result<bool,String>{c.current_dir(cwd);Ok(c.status().map_err(|e|format!("could not start command: {e}"))?.success())}
fn find_on_path(name:&str)->Option<PathBuf>{let p=env::var_os("PATH")?;for d in env::split_paths(&p){let x=d.join(name);if x.is_file(){return Some(x)}}None}
pub fn find_fusion(explicit:Option<&Path>)->Result<PathBuf,String>{
    if let Some(p)=explicit{return Ok(p.to_path_buf())} if let Ok(p)=env::var("FUSION_COMPILER"){return Ok(p.into())}
    let name=if cfg!(windows){"fusion.exe"}else{"fusion"}; if let Some(p)=find_on_path(name){return Ok(p)}
    if let Ok(me)=env::current_exe(){for base in me.ancestors().take(5){for p in [base.join("fusion"),base.join("target/debug/fusion"),base.join("target/release/fusion")]{if p.is_file(){return Ok(p)}}}}
    Err("could not find Fusion compiler; set FUSION_COMPILER or put fusion on PATH".into())
}
fn collect(dir:&Path,out:&mut Vec<PathBuf>){let Ok(rd)=fs::read_dir(dir)else{return};for e in rd.flatten(){let p=e.path();if p.is_dir(){collect(&p,out)}else if p.extension()==Some(OsStr::new("fusion")){out.push(p)}}}
pub fn source_files(p:&Project)->Vec<PathBuf>{let mut v=Vec::new();collect(&p.root.join("src"),&mut v);v.sort();v}
pub fn test_files(p:&Project)->Vec<PathBuf>{let mut v=Vec::new();collect(&p.root.join("tests"),&mut v);v.sort();v}
pub fn main_source(p:&Project)->Result<PathBuf,String>{let x=p.root.join("src/main.fusion");if x.is_file(){Ok(x)}else{source_files(p).into_iter().next().ok_or_else(||"no .fusion source files found in src/".into())}}

pub fn build(p:&Project,profile:BuildProfile,fusion:Option<&Path>,extra:&[String])->Result<PathBuf,String>{resolve_dependencies(p,false)?;let src=main_source(p)?;let dir=p.root.join("target").join(profile.name());fs::create_dir_all(&dir).map_err(|e|e.to_string())?;let out=dir.join(if cfg!(windows){format!("{}.exe",p.manifest.package.name)}else{p.manifest.package.name.clone()});let compiler=find_fusion(fusion)?;let mut c=Command::new(compiler);c.current_dir(&p.root).args(["build"]).arg(src).args(["-o"]).arg(&out).args(extra);if !run(&mut c,&p.root)?{return Err("Fusion compilation failed".into())}Ok(out)}
pub fn check(p:&Project,fusion:Option<&Path>)->Result<(),String>{let compiler=find_fusion(fusion)?;let files=source_files(p);if files.is_empty(){return Err("no .fusion source files found in src/".into())}for src in files{let mut c=Command::new(&compiler);c.current_dir(&p.root).args(["check"]).arg(&src);if !run(&mut c,&p.root)?{return Err(format!("check failed for {}",src.display()))}}Ok(())}
pub fn run_program(p:&Project,fusion:Option<&Path>,args:&[String])->Result<(),String>{let compiler=find_fusion(fusion)?;let mut c=Command::new(compiler);c.current_dir(&p.root).args(["run"]).arg(main_source(p)?).args(args);if !run(&mut c,&p.root)?{return Err("Fusion program exited with failure".into())}Ok(())}
pub fn test(p:&Project,fusion:Option<&Path>)->Result<(),String>{let compiler=find_fusion(fusion)?;let files=test_files(p);if files.is_empty(){println!("Foundry: no integration tests found in tests/");return Ok(())}let mut failed=0;for src in &files{print!("test {} ... ",src.strip_prefix(&p.root).unwrap_or(src).display());io::stdout().flush().ok();let mut c=Command::new(&compiler);c.current_dir(&p.root).args(["check"]).arg(src).stdout(Stdio::null());if c.status().map_err(|e|e.to_string())?.success(){println!("ok")}else{println!("FAILED");failed+=1}}if failed==0{println!("test result: ok. {} files checked",files.len());Ok(())}else{Err(format!("test result: FAILED. {failed} test files failed"))}}
pub fn clean(p:&Project)->Result<(),String>{let t=p.root.join("target");if t.exists(){fs::remove_dir_all(t).map_err(|e|e.to_string())?}Ok(())}
pub fn tree(p:&Project)->String{let mut s=format!("{} v{}\n",p.manifest.package.name,p.manifest.package.version);for(n,d)in&p.manifest.dependencies{s.push_str(&format!("├── {n} {}\n",label(d)))}for(n,d)in&p.manifest.dev_dependencies{s.push_str(&format!("[dev]── {n} {}\n",label(d)))}s}
fn label(d:&Dependency)->String{match d{Dependency::Version(v)=>v.clone(),Dependency::Path(p)=>format!("path:{}",p.display()),Dependency::Git{url,rev}=>format!("git:{url}{}",rev.as_ref().map(|x|format!("#{x}")).unwrap_or_default())}}

#[cfg(test)]mod tests{use super::*;#[test]fn manifest_parses(){let m=parse_manifest("[package]\nname=\"x\"\n[dependencies]\na=\"1\"\nb={ path=\"../b\" }\nc={ git=\"https://x/c.git\", rev=\"abc\" }\n").unwrap();assert_eq!(m.package.name,"x");assert!(matches!(m.dependencies["a"],Dependency::Version(_)));assert!(matches!(m.dependencies["b"],Dependency::Path(_)));assert!(matches!(m.dependencies["c"],Dependency::Git{..}));}}
