// build.rs
fn main() {
    // Point to your local sqlite amalgamation folder path
    let sqlite_dir = "sqlite-amalgamation-3530400";
    let sqlite_c = format!("{}/sqlite3.c", sqlite_dir);

    // Tell cargo to invalidate the built crate if the C source changes
    println!("cargo:rerun-if-changed={}", sqlite_c);

    // Compile sqlite3.c into a static library using cc or direct clang command
    cc::Build::new()
        .file(sqlite_c)
        .include(sqlite_dir)
        .flag_if_supported("-DSQLITE_OMIT_LOAD_EXTENSION=1") // Optional: security/simplicity flag
        .compile("sqlite3");
}