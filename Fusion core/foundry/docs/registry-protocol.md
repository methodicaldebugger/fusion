# Foundry registry protocol (initial implementation)

Set `FOUNDRY_REGISTRY` to an HTTPS URL or a local directory. For package `NAME`, the registry must provide:

- `packages/NAME/versions.txt`: one version per line.
- `packages/NAME/VERSION.toml`: versioned metadata with `format_version = 1`, `name`, `version`, `archive`, and `checksum` fields.
- `archive` points to a tar archive using HTTPS, an absolute filesystem path, or an absolute `file://` URL; `checksum` is the lowercase or uppercase SHA-256 digest of the exact archive bytes.

The metadata format version is mandatory. Foundry rejects missing, non-integer, or unsupported versions and requires the metadata `name` and `version` to exactly match the requested package identity. Unknown additional TOML fields are ignored in format 1 so registries can add non-semantic annotations without changing resolver behavior. A future incompatible schema must use a new `format_version` and a Foundry release that supports it.

Network registries and archive downloads must use HTTPS. Plain HTTP, FTP, embedded URL credentials, fragments, relative archive paths, and other URL schemes are rejected. HTTPS redirects are restricted to HTTPS. Local filesystem registries remain supported for development and deterministic integration tests. Registry version-index and package-metadata cache writes are staged and atomically published only after successful retrieval and validation; a failed request or malformed response leaves the previous cache record intact. Network requests have bounded connection/overall timeouts and a small retry budget, with curl diagnostics included in errors.

Example metadata:

```toml
format_version = 1
name = "math"
version = "1.2.0"
archive = "https://registry.example/packages/math/1.2.0.tar"
checksum = "<64 hexadecimal SHA-256 characters>"
```

The current resolver supports basic numeric semantic-version constraints: exact (`=1.2.0`), caret (`^1.2.0`), tilde (`~1.2.0`), comparisons, and `*`. It selects the highest matching numeric version. Prerelease ordering, registry authentication, transitive dependency resolution, and registry publishing are not yet implemented. Archives should be trusted and contain package source files only.

## Extracted package cache

Registry archive and extracted-directory integrity checks are described in [registry-cache-integrity.md](registry-cache-integrity.md). Archives must contain relative paths and regular files/directories only; symlinks, hard links, absolute paths, parent traversal, and special files are rejected before extraction.
