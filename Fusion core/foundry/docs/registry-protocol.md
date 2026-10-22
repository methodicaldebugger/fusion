# Foundry registry protocol (initial implementation)

Set `FOUNDRY_REGISTRY` to an HTTPS URL or a local directory. For package `NAME`, the registry must provide:

- `packages/NAME/versions.txt`: one version per line.
- `packages/NAME/VERSION.toml`: `name`, `version`, `archive`, and `checksum` fields.
- `archive` points to a tar archive (for local registries, use an absolute filesystem path or `file://` URL); `checksum` is the lowercase or uppercase SHA-256 digest of the exact archive bytes.

Example metadata:

```toml
name = "math"
version = "1.2.0"
archive = "https://registry.example/packages/math/1.2.0.tar"
checksum = "<64 hexadecimal SHA-256 characters>"
```

The current resolver supports basic numeric semantic-version constraints: exact (`=1.2.0`), caret (`^1.2.0`), tilde (`~1.2.0`), comparisons, and `*`. It selects the highest matching numeric version. Prerelease ordering, registry authentication, transitive dependency resolution, and registry publishing are not yet implemented. Archives should be trusted and contain package source files only.
