//contents of fingerprint.rs

use sha2::{Digest, Sha256};
use std::{fs, io, path::Path};

/// SHA-256 digest of a file's contents.
pub fn file_sha256(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

/// Hash multiple fields without ambiguous concatenation.
pub fn fingerprint_fields<'a, I>(fields: I) -> String
where
    I: IntoIterator<Item = &'a [u8]>,
{
    let mut hasher = Sha256::new();

    for field in fields {
        hasher.update((field.len() as u64).to_le_bytes());
        hasher.update(field);
    }

    hex::encode(hasher.finalize())
}
