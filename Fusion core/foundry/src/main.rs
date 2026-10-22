//contents of main.rs

use std::{env, fs, path::PathBuf, process::ExitCode};

fn usage() {
    println!("Foundry 0.1.0 - Fusion project/package/toolchain manager\n\nCommands:\n  foundry new <name>\n  foundry init\n  foundry build [--release] [--target <triple> ...]\n  foundry run [-- <args...>]\n  foundry check\n  foundry test\n  foundry fetch [--offline]\n  foundry tree\n  foundry metadata\n  foundry clean\n  foundry add <name> [version | --path PATH | --git URL]\n  foundry remove <name>\n  foundry fmt [--check]\n  foundry clippy\n  foundry doc\n  foundry version\n\nSet FUSION_COMPILER to select the Fusion executable.")
}

fn cwd() -> Result<PathBuf, String> {
    env::current_dir().map_err(|e| e.to_string())
}

fn project() -> Result<foundry::Project, String> {
    foundry::load_project(&foundry::project_root(&cwd()?).map_err(|e| e.to_string())?)
}

fn main() -> ExitCode {
    let mut a = env::args().skip(1).collect::<Vec<_>>();
    let cmd = a.first().cloned().unwrap_or_else(|| "help".into());
    if !a.is_empty() {
        a.remove(0);
    }
    let r = match cmd.as_str() {
        "help" | "-h" | "--help" => {
            usage();
            Ok(())
        }
        "version" | "-V" | "--version" => {
            println!("foundry 0.1.0");
            Ok(())
        }
        "new" => new_pkg(&a),
        "init" => init(),
        "build" => build(&a),
        "run" => run(&a),
        "check" => check(),
        "test" => test(),
        "fetch" => fetch(&a, false),
        "update" => fetch(&a, true),
        "tree" => tree(),
        "metadata" => metadata(),
        "clean" => clean(),
        "add" => add(&a),
        "remove" => remove(&a),
        "fmt" => fmt(&a),
        "clippy" => clippy(),
        "doc" => doc(),
        x => Err(format!("unknown command '{x}'. Run 'foundry help'.")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn new_pkg(args: &[String]) -> Result<(), String> {
    let Some(name) = args.first() else {
        return Err("usage: foundry new <project-name>".into());
    };

    if args.len() != 1 {
        return Err("usage: foundry new <project-name>".into());
    }

    let parent = std::env::current_dir()
        .map_err(|error| format!("cannot determine current directory: {error}"))?;

    let project =
        foundry::template::create_project(&parent, name).map_err(|error| error.to_string())?;

    println!("Created Fusion project `{name}`");
    println!();
    println!("  {}", project.join("Foundry.toml").display());
    println!("  {}", project.join("src/main.fusion").display());
    println!();
    println!("Next steps:");
    println!("  cd {name}");
    println!("  foundry check");
    println!("  foundry run");

    Ok(())
}

fn init() -> Result<(), String> {
    let r = cwd()?;
    if r.join(foundry::MANIFEST).exists() || r.join(foundry::LEGACY_MANIFEST).exists() {
        return Err("Foundry.toml (or legacy fusion.toml) already exists".into());
    }
    let n = r
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("fusion-app");
    fs::write(r.join(foundry::MANIFEST), foundry::manifest_template(n))
        .map_err(|e| e.to_string())?;
    let p = foundry::load_project(&r)?;
    foundry::ensure_layout(&p)?;
    println!("Initialized Fusion package in {}", r.display());
    Ok(())
}

fn build(a: &[String]) -> Result<(), String> {
    let p = project()?;
    let release = a.iter().any(|x| x == "--release");
    let profile = if release {
        foundry::BuildProfile::Release
    } else {
        foundry::BuildProfile::Debug
    };
    let extra = a
        .iter()
        .filter(|x| x.as_str() != "--release")
        .cloned()
        .collect::<Vec<_>>();
    let targets = extra
        .windows(2)
        .filter(|w| w[0] == "--target")
        .map(|w| w[1].clone())
        .collect::<Vec<_>>();
    if targets.len() > 1 {
        let mut non_target = Vec::new();
        let mut skip = false;
        for item in &extra {
            if skip {
                skip = false;
                continue;
            }
            if item == "--target" {
                skip = true;
                continue;
            }
            non_target.push(item.clone());
        }
        let outputs = foundry::build_targets(&p, profile, None, &targets, &non_target)?;
        println!(
            "Finished {} build for {} targets",
            profile.name(),
            outputs.len()
        );
    } else {
        let out = foundry::build(&p, profile, None, &extra)?;
        println!("Finished {} build: {}", profile.name(), out.display());
    }
    Ok(())
}

fn run(a: &[String]) -> Result<(), String> {
    let args = a
        .iter()
        .position(|x| x == "--")
        .map(|i| a[i + 1..].to_vec())
        .unwrap_or_default();
    foundry::run_program(&project()?, None, &args)
}

fn check() -> Result<(), String> {
    let p = project()?;
    foundry::check(&p, None)?;
    println!("Finished check");
    Ok(())
}

fn test() -> Result<(), String> {
    let p = project()?;
    foundry::test(&p, None)
}

fn metadata() -> Result<(), String> {
    let p = project()?;
    println!("{{\n  \"name\": \"{}\",\n  \"version\": \"{}\",\n  \"root\": \"{}\",\n  \"sources\": {},\n  \"tests\": {},\n  \"dependencies\": {}\n}}",p.manifest.package.name,p.manifest.package.version,p.root.display(),foundry::source_files(&p).len(),foundry::test_files(&p).len(),p.manifest.dependencies.len());
    Ok(())
}

fn fetch(a: &[String], update: bool) -> Result<(), String> {
    let p = project()?;
    foundry::resolve_dependencies_with_update(&p, a.iter().any(|x| x == "--offline"), update)?;
    println!("Dependencies resolved");
    Ok(())
}

fn tree() -> Result<(), String> {
    let p = project()?;
    print!("{}", foundry::tree(&p));
    Ok(())
}

fn clean() -> Result<(), String> {
    let p = project()?;
    foundry::clean(&p)?;
    println!("Removed target/");
    Ok(())
}

fn add(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: foundry add <name> [version | --path PATH | --git URL]".into());
    }
    let name = &args[0];
    if !valid_dependency_name(name) {
        return Err(format!("invalid dependency name '{name}'"));
    }
    let mut section = "dependencies";
    let mut spec = String::from("\"*\"");
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dev" => section = "dev-dependencies",
            "--path" if i + 1 < args.len() => {
                spec = format!("{{ path = {:?} }}", args[i + 1]);
                i += 1;
            }
            "--git" if i + 1 < args.len() => {
                spec = format!("{{ git = {:?} }}", args[i + 1]);
                i += 1;
            }
            "--version" if i + 1 < args.len() => {
                spec = format!("{:?}", args[i + 1]);
                i += 1;
            }
            value if !value.starts_with('-') => spec = format!("{value:?}"),
            flag => return Err(format!("unknown or incomplete option '{flag}'")),
        }
        i += 1;
    }
    let p = project()?;
    let path = if p.root.join(foundry::MANIFEST).is_file() {
        p.root.join(foundry::MANIFEST)
    } else {
        p.root.join(foundry::LEGACY_MANIFEST)
    };
    let mut text = fs::read_to_string(&path).map_err(|e| format!("read manifest: {e}"))?;
    if dependency_exists(&text, name) {
        return Err(format!("dependency '{name}' is already declared"));
    }
    text = insert_manifest_entry(&text, section, name, &spec)?;
    fs::write(&path, text).map_err(|e| format!("write manifest: {e}"))?;
    foundry::load_project(&p.root).and_then(|project| foundry::write_lock(&project))?;
    println!("Added {name} to [{section}]");
    Ok(())
}

fn remove(args: &[String]) -> Result<(), String> {
    if args.len() != 1 {
        return Err("usage: foundry remove <name>".into());
    }
    let name = &args[0];
    let p = project()?;
    let path = if p.root.join(foundry::MANIFEST).is_file() {
        p.root.join(foundry::MANIFEST)
    } else {
        p.root.join(foundry::LEGACY_MANIFEST)
    };
    let text = fs::read_to_string(&path).map_err(|e| format!("read manifest: {e}"))?;
    let (updated, removed) = remove_manifest_entry(&text, name);
    if !removed {
        return Err(format!("dependency '{name}' was not found"));
    }
    fs::write(&path, updated).map_err(|e| format!("write manifest: {e}"))?;
    let project = foundry::load_project(&p.root)?;
    foundry::write_lock(&project)?;
    println!("Removed {name}");
    Ok(())
}

fn valid_dependency_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
fn dependency_exists(text: &str, name: &str) -> bool {
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        } else if matches!(
            section.as_str(),
            "dependencies" | "dev-dependencies" | "plugins"
        ) && line
            .split_once('=')
            .map(|(k, _)| k.trim() == name)
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}
fn insert_manifest_entry(
    text: &str,
    section: &str,
    name: &str,
    spec: &str,
) -> Result<String, String> {
    let header = format!("[{section}]");
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    if let Some(start) = lines.iter().position(|l| l.trim() == header) {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, l)| l.trim().starts_with('[') && l.trim().ends_with(']'))
            .map(|(i, _)| i)
            .unwrap_or(lines.len());
        lines.insert(end, format!("{name} = {spec}"));
    } else {
        if !lines.is_empty() && !lines.last().unwrap().is_empty() {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(format!("{name} = {spec}"));
    }
    Ok(lines.join("\n") + "\n")
}
fn remove_manifest_entry(text: &str, name: &str) -> (String, bool) {
    let mut section = String::new();
    let mut removed = false;
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        }
        if !removed
            && matches!(
                section.as_str(),
                "dependencies" | "dev-dependencies" | "plugins"
            )
            && line
                .split_once('=')
                .map(|(k, _)| k.trim() == name)
                .unwrap_or(false)
        {
            removed = true;
            continue;
        }
        out.push(raw.to_string());
    }
    (out.join("\n") + "\n", removed)
}

fn fmt(args: &[String]) -> Result<(), String> {
    let p = project()?;
    let compiler = foundry::find_project_fusion(&p, None)?;
    let mut command = std::process::Command::new(compiler);
    command.current_dir(&p.root).arg("fmt");
    if args.iter().any(|a| a == "--check") {
        command.arg("--check");
    }
    let status = command
        .status()
        .map_err(|e| format!("could not start Fusion formatter: {e}"))?;
    if !status.success() {
        return Err("Fusion formatter failed (ensure your compiler supports `fusion fmt`)".into());
    }
    Ok(())
}
fn clippy() -> Result<(), String> {
    let p = project()?;
    let compiler = foundry::find_project_fusion(&p, None)?;
    let mut command = std::process::Command::new(compiler);
    command.current_dir(&p.root).arg("analyze");
    let status = command
        .status()
        .map_err(|e| format!("could not start Fusion analyser: {e}"))?;
    if !status.success() {
        return Err(
            "Fusion static analysis failed (ensure your compiler supports `fusion analyze`)".into(),
        );
    }
    Ok(())
}
fn doc() -> Result<(), String> {
    let p = project()?;
    let docs = p.root.join("target/doc");
    fs::create_dir_all(&docs).map_err(|e| format!("create documentation directory: {e}"))?;
    let mut out = format!(
        "# {}\n\nVersion: `{}`\n\n",
        p.manifest.package.name, p.manifest.package.version
    );
    if let Some(description) = &p.manifest.package.description {
        out.push_str(description);
        out.push_str("\n\n");
    }
    out.push_str("## Source files\n\n");
    for src in foundry::source_files(&p) {
        out.push_str(&format!(
            "- `{}`\n",
            src.strip_prefix(&p.root).unwrap_or(&src).display()
        ));
    }
    out.push_str("\n## Dependencies\n\n");
    for (name, dep) in &p.manifest.dependencies {
        out.push_str(&format!(
            "- `{name}` — {}\n",
            match dep {
                foundry::Dependency::Version(v) => v.clone(),
                foundry::Dependency::Path(path) => format!("path `{}`", path.display()),
                foundry::Dependency::Git { url, rev } => format!(
                    "git `{url}`{}",
                    rev.as_ref()
                        .map(|r| format!(" at revision `{r}`"))
                        .unwrap_or_default()
                ),
            }
        ));
    }
    fs::write(docs.join("README.md"), out).map_err(|e| format!("write docs: {e}"))?;
    println!("Generated {}", docs.join("README.md").display());
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn add_inserts_into_existing_dependency_section() {
        let input = "[package]\nname = \"demo\"\n\n[dependencies]\nfoo = \"1\"\n";
        let output = insert_manifest_entry(input, "dependencies", "bar", "\"2\"").unwrap();
        assert!(output.contains("bar = \"2\""));
        assert!(dependency_exists(&output, "bar"));
    }

    #[test]
    fn add_creates_missing_dev_dependency_section() {
        let input = "[package]\nname = \"demo\"\n";
        let output =
            insert_manifest_entry(input, "dev-dependencies", "test-helper", "\"*\"").unwrap();
        assert!(output.contains("[dev-dependencies]"));
        assert!(output.contains("test-helper = \"*\""));
    }

    #[test]
    fn remove_only_removes_matching_dependency() {
        let input = "[dependencies]\nfoo = \"1\"\nbar = \"2\"\n";
        let (output, removed) = remove_manifest_entry(input, "foo");
        assert!(removed);
        assert!(!dependency_exists(&output, "foo"));
        assert!(dependency_exists(&output, "bar"));
    }
}
