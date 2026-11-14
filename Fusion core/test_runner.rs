//! `fusion test` and `fusion bench`: discovery and execution of `#[test]` / `#[bench]` functions.
//!
//! Tests are ordinary zero-argument Fusion functions marked with `#[test]`. Each test runs in a
//! fresh interpreter (top-level statements are executed, `main` is not), so tests cannot affect
//! one another. A test fails when it panics: failed `assert*` calls, runtime errors and
//! `test_fail(...)` all count. `#[should_panic]` inverts that, `#[ignore]` skips a test unless
//! `--ignored` is given, and `#[bench]` functions are timed by `fusion bench`.

use crate::ast::Statement;
use crate::conc_ext::panic_message;
use crate::interpreter::Interpreter;
use std::panic::{self, AssertUnwindSafe};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Test,
    Bench,
}

#[derive(Debug, Clone)]
pub struct Case {
    /// Display name (`name` in the entry module, `module::name` elsewhere).
    pub name: String,
    /// Name passed to the interpreter (resolved through module lowering).
    pub symbol: String,
    pub kind: Kind,
    pub ignore: bool,
    pub should_panic: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Options {
    pub filter: Option<String>,
    pub include_ignored: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Summary {
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
}

fn function_name(line: &str) -> Option<String> {
    let mut rest = line.trim();
    if let Some(r) = rest.strip_prefix("pub ") {
        rest = r.trim_start();
    }
    if let Some(r) = rest.strip_prefix("async ") {
        rest = r.trim_start();
    }
    let rest = rest.strip_prefix("fn ")?;
    let name: String = rest.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Finds `#[test]` and `#[bench]` functions in Fusion source text.
pub fn discover(source: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    let (mut test, mut bench, mut ignore, mut should_panic) = (false, false, false, false);
    for raw in source.lines() {
        let line = raw.trim();
        if let Some(inner) = line.strip_prefix("#[") {
            let inner = inner.trim_end_matches(']');
            let name = inner.split('(').next().unwrap_or("").trim();
            match name {
                "test" => test = true,
                "bench" => bench = true,
                "ignore" => ignore = true,
                "should_panic" => should_panic = true,
                _ => {}
            }
            continue;
        }
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        if let Some(name) = function_name(line) {
            if test || bench {
                cases.push(Case {
                    symbol: name.clone(),
                    name,
                    kind: if test { Kind::Test } else { Kind::Bench },
                    ignore,
                    should_panic,
                });
            }
        }
        test = false;
        bench = false;
        ignore = false;
        should_panic = false;
    }
    cases
}

fn load(path: &str) -> Result<(crate::ast::Program, Vec<Case>), String> {
    let project = crate::compile_project_with_metadata(path)?;
    let mut program = project.program.clone();
    program.statements.retain(|s| !matches!(s, Statement::Main { .. }));
    let mut cases: Vec<Case> = Vec::new();
    for module in &project.modules {
        if let Ok(source) = std::fs::read_to_string(&module.path) {
            for mut case in discover(&source) {
                if module.name != "crate" {
                    case.symbol = format!("{}.{}", module.name, case.name);
                    case.name = format!("{}::{}", module.name, case.name);
                }
                if !cases.iter().any(|c| c.name == case.name) {
                    cases.push(case);
                }
            }
        }
    }
    Ok((program, cases))
}

fn matches_filter(name: &str, options: &Options) -> bool {
    options.filter.as_ref().map_or(true, |f| name.contains(f.as_str()))
}

fn run_once(program: &crate::ast::Program, name: &str) -> (Result<(), String>, Vec<String>) {
    let mut interp = Interpreter::new();
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        interp.execute(program);
        if !interp.has_function(name) {
            panic!("function '{}' was not found in the compiled program", name);
        }
        interp.call_function_by_name(name, Vec::new());
    }));
    let output = interp.take_output();
    (result.map_err(panic_message), output)
}

/// Runs every `#[test]` function reachable from `path`. Prints results and returns a summary.
pub fn run_tests(path: &str, options: &Options) -> Result<Summary, String> {
    let (program, cases) = load(path)?;
    let tests: Vec<&Case> = cases.iter().filter(|c| c.kind == Kind::Test && matches_filter(&c.name, options)).collect();
    println!("running {} test(s)", tests.len());
    let started = Instant::now();
    let mut summary = Summary::default();
    let mut failures: Vec<(String, String, Vec<String>)> = Vec::new();
    panic::set_hook(Box::new(|_| {}));
    for case in tests {
        if case.ignore && !options.include_ignored {
            println!("test {} ... ignored", case.name);
            summary.ignored += 1;
            continue;
        }
        let (result, output) = run_once(&program, &case.symbol);
        let passed = match (&result, case.should_panic) {
            (Ok(()), false) => true,
            (Err(_), true) => true,
            _ => false,
        };
        if passed {
            println!("test {} ... ok", case.name);
            summary.passed += 1;
        } else {
            println!("test {} ... FAILED", case.name);
            summary.failed += 1;
            let reason = match result {
                Err(message) => message,
                Ok(()) => "test did not panic as #[should_panic] requires".to_string(),
            };
            failures.push((case.name.clone(), reason, output));
        }
    }
    let _ = panic::take_hook();
    if !failures.is_empty() {
        println!("\nfailures:");
        for (name, reason, output) in &failures {
            println!("\n---- {} ----", name);
            for line in output {
                println!("{}", line);
            }
            println!("{}", reason);
        }
    }
    println!(
        "\ntest result: {}. {} passed; {} failed; {} ignored; finished in {} ms",
        if summary.failed == 0 { "ok" } else { "FAILED" },
        summary.passed,
        summary.failed,
        summary.ignored,
        started.elapsed().as_millis()
    );
    Ok(summary)
}

/// Times every `#[bench]` function: one warm-up call, then repeated calls for about 300 ms.
pub fn run_benchmarks(path: &str, options: &Options) -> Result<usize, String> {
    let (program, cases) = load(path)?;
    let benches: Vec<&Case> = cases.iter().filter(|c| c.kind == Kind::Bench && matches_filter(&c.name, options)).collect();
    println!("running {} benchmark(s)", benches.len());
    panic::set_hook(Box::new(|_| {}));
    let mut failed = 0usize;
    for case in &benches {
        let mut interp = Interpreter::new();
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
            interp.execute(&program);
            interp.call_function_by_name(&case.symbol, Vec::new());
            let budget = Duration::from_millis(300);
            let begin = Instant::now();
            let (mut iterations, mut fastest) = (0u64, u128::MAX);
            while begin.elapsed() < budget && iterations < 100_000 {
                let t = Instant::now();
                interp.call_function_by_name(&case.symbol, Vec::new());
                fastest = fastest.min(t.elapsed().as_nanos());
                iterations += 1;
            }
            (iterations, fastest, begin.elapsed().as_nanos())
        }));
        match outcome {
            Ok((iterations, fastest, total)) if iterations > 0 => {
                println!("bench {} ... {} ns/iter (min {} ns, {} iterations)", case.name, total / iterations as u128, fastest, iterations)
            }
            Ok(_) => println!("bench {} ... too slow to measure", case.name),
            Err(payload) => {
                failed += 1;
                println!("bench {} ... FAILED: {}", case.name, panic_message(payload));
            }
        }
    }
    let _ = panic::take_hook();
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_attributes() {
        let src = "#[test]\nfn adds():\n    assert(1 + 1 == 2)\n\n#[test]\n#[ignore]\nasync fn slow():\n    assert(true)\n\n#[derive(Debug)]\nstruct P:\n    x: num\n\n#[bench]\npub fn speed():\n    x = 1\n\nfn helper():\n    x = 1\n\n#[test]\n#[should_panic(expected = \"boom\")]\nfn boom():\n    test_fail(\"boom\")\n";
        let cases = discover(src);
        let names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["adds", "slow", "speed", "boom"]);
        assert!(cases[1].ignore);
        assert_eq!(cases[2].kind, Kind::Bench);
        assert!(cases[3].should_panic);
    }
}
