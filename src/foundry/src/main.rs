/* Foundry - Fusion project/package/toolchain manager

The package manager for Fusion projects. It provides commands to create, build, run, and manage Fusion packages and their dependencies.
It is responsible managing dependency resolution, building Fusion programs, running and testing them, compatibility checks, publishing and more.
It resolves version, dependencies and compatibility issues for plugins.
*/


use std::{env,fs,path::PathBuf,process::ExitCode};
fn usage(){println!("Foundry 0.1.0 - Fusion project/package/toolchain manager\n\nCommands:\n  foundry new <name>\n  foundry init\n  foundry build [--release]\n  foundry run [-- <args...>]\n  foundry check\n  foundry test\n  foundry fetch [--offline]\n  foundry tree\n  foundry metadata\n  foundry clean\n  foundry version\n\nSet FUSION_COMPILER to select the Fusion executable.")}
fn cwd()->Result<PathBuf,String>{env::current_dir().map_err(|e|e.to_string())}
fn project()->Result<foundry::Project,String>{foundry::load_project(&foundry::project_root(&cwd()?).map_err(|e|e.to_string())?)}
fn main()->ExitCode{let mut a=env::args().skip(1).collect::<Vec<_>>();let cmd=a.first().cloned().unwrap_or_else(||"help".into());if !a.is_empty(){a.remove(0)}let r=match cmd.as_str(){"help"|"-h"|"--help"=>{usage();Ok(())},"version"|"-V"|"--version"=>{println!("foundry 0.1.0");Ok(())},"new"=>new_pkg(&a),"init"=>init(),"build"=>build(&a),"run"=>run(&a),"check"=>check(),"test"=>test(),"fetch"|"update"=>fetch(&a),"tree"=>tree(),"metadata"=>metadata(),"clean"=>clean(),x=>Err(format!("unknown command '{x}'. Run 'foundry help'."))};match r{Ok(())=>ExitCode::SUCCESS,Err(e)=>{eprintln!("error: {e}");ExitCode::from(1)}}}
fn new_pkg(a:&[String])->Result<(),String>{let name=a.iter().find(|x|!x.starts_with('-')).ok_or("foundry new requires a package name")?;if name.contains('/')||name.contains('\\')||name=="."||name==".."{return Err("package name must be a simple directory/name".into())}let r=cwd()?.join(name);if r.exists(){return Err(format!("destination already exists: {}",r.display()))}fs::create_dir_all(r.join("src")).map_err(|e|e.to_string())?;fs::create_dir_all(r.join("tests")).map_err(|e|e.to_string())?;fs::create_dir_all(r.join("foreign")).map_err(|e|e.to_string())?;fs::write(r.join("fusion.toml"),foundry::manifest_template(name)).map_err(|e|e.to_string())?;fs::write(r.join("src/main.fusion"),"main:\n    print(\"Hello, Fusion!\")\n").map_err(|e|e.to_string())?;println!("Created Fusion package '{name}'");Ok(())}
fn init()->Result<(),String>{let r=cwd()?;if r.join("fusion.toml").exists(){return Err("fusion.toml already exists".into())}let n=r.file_name().and_then(|x|x.to_str()).unwrap_or("fusion-app");fs::write(r.join("fusion.toml"),foundry::manifest_template(n)).map_err(|e|e.to_string())?;let p=foundry::load_project(&r)?;foundry::ensure_layout(&p)?;println!("Initialized Fusion package in {}",r.display());Ok(())}
fn build(a:&[String])->Result<(),String>{let p=project()?;let release=a.iter().any(|x|x=="--release");let profile=if release{foundry::BuildProfile::Release}else{foundry::BuildProfile::Debug};let extra=a.iter().filter(|x|x.as_str()!="--release").cloned().collect::<Vec<_>>();let out=foundry::build(&p,profile,None,&extra)?;println!("Finished {} build: {}",profile.name(),out.display());Ok(())}
fn run(a:&[String])->Result<(),String>{let args=a.iter().position(|x|x=="--").map(|i|a[i+1..].to_vec()).unwrap_or_default();foundry::run_program(&project()?,None,&args)}
fn check()->Result<(),String>{let p=project()?;foundry::check(&p,None)?;println!("Finished check");Ok(())}
fn test()->Result<(),String>{let p=project()?;foundry::test(&p,None)}
fn metadata()->Result<(),String>{let p=project()?;println!("{{\n  \"name\": \"{}\",\n  \"version\": \"{}\",\n  \"root\": \"{}\",\n  \"sources\": {},\n  \"tests\": {},\n  \"dependencies\": {}\n}}",p.manifest.package.name,p.manifest.package.version,p.root.display(),foundry::source_files(&p).len(),foundry::test_files(&p).len(),p.manifest.dependencies.len());Ok(())}

fn fetch(a:&[String])->Result<(),String>{let p=project()?;foundry::resolve_dependencies(&p,a.iter().any(|x|x=="--offline"))?;println!("Dependencies resolved");Ok(())}
fn tree()->Result<(),String>{let p=project()?;print!("{}",foundry::tree(&p));Ok(())}
fn clean()->Result<(),String>{let p=project()?;foundry::clean(&p)?;println!("Removed target/");Ok(())}
