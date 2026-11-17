//! C language integration plugin for Fusion.
//! Used by the cloud builds plugin to compile and execute C code.

use fusion_plugin_c::CPluginBridge;
use std::path::Path;
use std::fs;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let source_file = Path::new("foreign/math_lib.c");
    let build_dir = Path::new("target/native");

    // Ensure directories exist
    fs::create_dir_all("foreign")?;
    fs::create_dir_all(build_dir)?;

    // Write a test C file if it doesn't already exist
    if !source_file.exists() {
        fs::write(
            source_file,
            r#"
            #ifdef _WIN32
            #define EXPORT __declspec(dllexport)
            #else
            #define EXPORT
            #endif

            EXPORT int add_numbers(int a, int b) {
                return a + b;
            }
            "#,
        )?;
    }

    // 1. Compile C file to DLL via Clang
    let dll_path = CPluginBridge::compile_c_source(source_file, build_dir)?;

    // 2. Load the library and execute the FFI call
    let bridge = CPluginBridge::new(dll_path);
    let result = bridge.call_add_numbers(40, 2)?;

    println!("--------------------------------------------------");
    println!("Milestone 1 Success! Result from C library: {}", result); // Prints 42
    println!("--------------------------------------------------");

    Ok(())
}