//! Small process-based integration test harness for Fusion source tests.
use std::{io::{self, Write}, path::Path, process::Command, time::Instant};
#[derive(Debug, Default)] pub struct TestSummary { pub passed:usize, pub failed:usize, pub elapsed_ms:u128 }
pub fn run_files(compiler:&Path, root:&Path, files:&[std::path::PathBuf])->Result<TestSummary,String>{
 let started=Instant::now(); let mut summary=TestSummary::default();
 for file in files { print!("test {} ... ",file.strip_prefix(root).unwrap_or(file).display()); let _=io::stdout().flush();
  let result=Command::new(compiler).current_dir(root).args(["run"]).arg(file).status().map_err(|e|format!("could not start test {}: {e}",file.display()))?;
  if result.success(){summary.passed+=1;println!("ok")}else{summary.failed+=1;println!("FAILED (exit {})",result.code().map(|x|x.to_string()).unwrap_or_else(||"signal".into()));}
 }
 summary.elapsed_ms=started.elapsed().as_millis(); println!("test result: {}. {} passed; {} failed; {} ms",if summary.failed==0{"ok"}else{"FAILED"},summary.passed,summary.failed,summary.elapsed_ms);
 if summary.failed==0{Ok(summary)}else{Err(format!("{} integration test file(s) failed",summary.failed))}
}
