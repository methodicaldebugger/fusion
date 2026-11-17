//! C language integration plugin for Fusion.
//! Used by the cloud builds plugin to compile and execute C code.

use std::process::Command;
use std::path::{Path, PathBuf};
use libloading::{Library, Symbol};
use std::error::Error;

pub struct CPluginBridge {
    lib_path: PathBuf,
}

impl CPluginBridge {
    /// Compiles a target C source file into a shared library using local Clang.
    pub fn compile_c_source(c_source_path: &Path, output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
        let file_stem = c_source_path.file_stem()
            .ok_or("Invalid C source file path")?
            .to_str()
            .ok_or("Invalid UTF-8 in filename")?;

        let output_filename = format!("{}.dll", file_stem);
        let output_path = output_dir.join(output_filename);

        println!("Compiling C source via Clang: {:?}", c_source_path);

        // Pointing directly to your local clang installation
        let clang_path = r"C:\llvm\bin\clang.exe";
        
        let status = Command::new(clang_path)
            .arg("-shared")
            .arg("-o")
            .arg(&output_path)
            .arg(c_source_path)
            .status()?;

        if !status.success() {
            return Err("Clang compilation failed".into());
        }

        println!("Successfully generated shared library: {:?}", output_path);
        Ok(output_path)
    }

    /// Initializes the bridge with the compiled shared library.
    pub fn new(lib_path: PathBuf) -> Self {
        Self { lib_path }
    }

    /// Dynamically loads and executes `add_numbers` exported by the C library.
    pub fn call_add_numbers(&self, a: i32, b: i32) -> Result<i32, Box<dyn Error>> {
        unsafe {
            let lib = Library::new(&self.lib_path)?;
            
            // Signature maps directly to: int add_numbers(int a, int b)
            let func: Symbol<unsafe extern "C" fn(i32, i32) -> i32> = lib.get(b"add_numbers")?;
            
            let result = func(a, b);
            Ok(result)
        }
    }
}