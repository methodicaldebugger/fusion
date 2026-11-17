//! contents of ffi_loader.rs

use std::process::Command;
use std::path::{Path, PathBuf};
use libloading::{Library, Symbol};
use std::error::Error;

pub struct CFFIBridge {
    lib_path: PathBuf,
}

impl CFFIBridge {
    /// Compiles a target C source file into a shared library using local Clang.
    pub fn compile_c_source(c_source_path: &Path, output_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
        let file_stem = c_source_path.file_stem()
            .ok_or("Invalid C source file path")?
            .to_str()
            .ok_or("Invalid UTF-8 in filename")?;

        // Determine output DLL path for Windows (or .so / .dylib for other platforms)
        let output_filename = format!("{}.dll", file_stem);
        let output_path = output_dir.join(output_filename);

        println!("Compiling C source using Clang: {:?}", c_source_path);

        // Explicitly point to your local clang installation if needed, or invoke system clang
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

    /// Loads the compiled dynamic library and prepares for execution.
    pub fn new(lib_path: PathBuf) -> Self {
        Self { lib_path }
    }

    /// Dynamically loads a symbol (e.g., add_numbers) and executes it with integer arguments.
    pub fn call_add_numbers(&self, a: i32, b: i32) -> Result<i32, Box<dyn Error>> {
        unsafe {
            // Load the dynamic library (.dll)
            let lib = Library::new(&self.lib_path)?;

            // Look up the exact function symbol exported by the C code
            // Type signature matches: int add_numbers(int, int)
            let func: Symbol<unsafe extern "C" fn(i32, i32) -> i32> = lib.get(b"add_numbers")?;

            // Execute the foreign C function safely through the ABI boundary
            let result = func(a, b);
            Ok(result)
        }
    }
}