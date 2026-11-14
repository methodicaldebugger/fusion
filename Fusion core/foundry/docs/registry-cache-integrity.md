# Registry package cache integrity

Foundry verifies the SHA-256 of each downloaded registry archive against the checksum in the resolved registry metadata/lockfile before it uses the archive. The extracted package directory is treated as a separate cache object and is checked independently.

For each extracted package, Foundry stores a sibling `<name>-<version>.tree.toml` record containing:

- `format_version = 1`
- `archive_sha256`: the checksum of the archive from which the tree was extracted
- `tree_sha256`: a deterministic digest over sorted relative paths, entry kinds, permission identity, and regular-file bytes

On every registry resolution Foundry hashes the extracted tree and compares it with this record. A missing, malformed, archive-mismatched, or tree-mismatched record causes Foundry to re-extract the checksum-verified archive into a unique staging directory. The staged tree and its manifest are published with sibling renames; the previous extracted directory and manifest are retained as backups until both new objects are published. If extraction or publication fails, the previous cache directory is restored.

Archive inspection rejects absolute paths, parent-directory traversal, Windows-style path separators/drive prefixes, symlinks, hard links, and special files before extraction. This avoids archive entries redirecting extraction outside the package cache. Consequently, registry archives must contain ordinary directories and regular files only; packages that require symlinks or special filesystem entries are not supported by this resolver yet.

The tree manifest is a local cache-integrity record, not a digital signature. It detects ordinary cache corruption or edits when the manifest remains intact. It is not intended to defend against an attacker who can modify both the extracted package and its local tree manifest. The locked archive checksum remains the trust anchor for the archive bytes.

Archive downloads are written to unique temporary siblings and renamed into the cache only after the transfer completes, so an interrupted transfer does not leave a partial file that looks cached. A malformed or tampered extracted tree is repaired from the already checksum-verified archive without contacting the registry, so this repair works offline. A corrupt archive is rejected and removed; the next online resolution can download it again. Failed extraction never replaces the last existing extracted directory.

## Concurrent resolution and cache pruning

Registry resolution takes an atomic workspace lock through lockfile publication and an atomic per-package cache lock before reading or publishing the version index, metadata, archive, extracted tree, and tree manifest. The workspace lock prevents pruning from observing an old lockfile between package population and publication of the new pins; the package lock also protects direct concurrent registry API calls. The lock is process-scoped and released on normal return. A lock whose recorded PID is demonstrably no longer running is reclaimed automatically; malformed or ambiguous lock records fail closed and eventually report a timeout with the lock path for manual recovery. This avoids time-based lock stealing while another process may still be downloading or extracting.

After a verified extracted tree is available, Foundry removes abandoned staging, backup, and temporary-manifest siblings for that exact package version. Cleanup is serialized by the same package lock and never sweeps another package's transaction files.

The registry cache lives under each workspace's `target/foundry/deps`, not in a shared global cache. Run `foundry cache prune` from a workspace to remove cached registry versions not referenced by that workspace's validated `foundry.lock`. Pruning retains all exact registry pins in the lockfile and holds the workspace lock while reading pins and deleting cache entries. It also takes each package lock before deletion and re-reads the lockfile after acquiring the lock. Other workspaces are unaffected because their cache directories are separate. Entries with malformed metadata are retained rather than guessed from ambiguous hyphenated filenames.


## Read-only cache inspection

`foundry cache inspect` scans registry metadata records and reports whether each archive checksum and extracted-tree integrity sidecar agree. It distinguishes malformed metadata and unsupported metadata schemas, invalid identities/checksums, missing or unreadable archives, archive checksum mismatches, missing extracted directories, invalid tree manifests, and extracted-tree mismatches. `foundry cache inspect --json` provides the same status records for automation. Inspection is read-only: it does not repair or remove files. Re-resolving a locked package with `foundry fetch` verifies and repairs an extracted-tree mismatch. If the archive itself is corrupt, the resolver removes the bad archive and reports the checksum failure; run `foundry fetch` again to download and verify a fresh copy. `foundry cache prune` removes only unreferenced versions from the current workspace cache.
