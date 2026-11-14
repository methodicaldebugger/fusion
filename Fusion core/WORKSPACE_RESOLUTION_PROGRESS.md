
## Phase 3 continuation: enum and nominal aggregate ABI v6

- Package interface format advanced to v6. Public enums now emit deterministic `abi-enum` records containing the stable nominal LLVM type identity, ordered variant names, explicit sequential discriminants, payload storage types, and a SHA-256 layout digest.
- Foundry validates enum record shape, unique variant names, ordered discriminants, payload storage types, and digest consistency before accepting package interfaces.
- Non-generic, synchronous function ABI extraction now recognizes module-local nominal struct/enum parameter and return types as pointer-represented ABI values. This records the current backend's pointer calling convention instead of incorrectly rejecting all nominal signatures.
- Aggregate function ABI support is deliberately limited to nominal references represented as pointers; by-value aggregate passing, generic instantiations, and complete enum construction/matching ABI lowering are not claimed complete.
- Added enum ABI validator regression coverage for valid payload layouts and invalid discriminant ordering. Existing scalar and struct interface tests were migrated to v6.
- Package artifact format remains v6 and LLVM package artifact kind remains `llvm-package-ir-v5`; interface format versioning is independent of the cached artifact record format.

## Phase 3 continuation: enforce emitted enum runtime layout

- Extended the pre-link package layout validation pass to inspect each provider's emitted LLVM identified type for every exported `abi-enum` record. The concrete type must be exactly `{ i32, ptr }`, matching the backend's tagged-enum representation (32-bit declaration-order discriminant plus pointer to a variant-specific payload record).
- The check runs alongside struct layout validation before package LLVM modules are passed to the native linker. It also participates in the existing cross-unit identified-type conflict check.
- Added a Foundry regression test that accepts the expected tagged-pointer enum layout and rejects a provider whose LLVM type drifts to `{ i64, ptr }` despite a valid interface record.
- This closes an important gap between enum metadata and the backend representation, but does not yet prove cross-package runtime construction/matching in a built Fusion program. Payload-record layout agreement across separately compiled providers/consumers and end-to-end executable package tests remain follow-up work.

## Enum payload ABI verification

The package interface now publishes a separate `abi-enum-payload` record for every exported enum variant. Each record carries the stable payload identified-type symbol, ordered storage fields, field count, and SHA-256 digest. Foundry validates the record and checks that the provider LLVM module actually emits that exact payload layout. Cross-module identified-type conflict detection now includes `%fusion.variant.*` payload types as well as `%fusion.type.*` nominal types. This closes the previous gap where only the outer `{ i32, ptr }` enum header was checked while payload storage could drift independently.

This is still a link-time ABI consistency check, not proof of a complete multi-package Fusion executable. End-to-end compiler/runtime tests remain outstanding; Rust tests could not be executed in this environment because `cargo` and `rustc` are unavailable.

## Cross-package aggregate runtime ABI regression coverage (November 2026)

- Package-interface function signature extraction now builds a project-wide nominal type-name set from the module graph. This includes stable lowered type symbols and module/import-alias-qualified source names, so a dependency type used in another package's parameter or return annotation is recorded as the existing opaque `ptr` ABI rather than incorrectly marked unsupported.
- Added a three-module compiler regression fixture: the `model` package owns a public struct and tagged enum, constructs a struct and exports it through a pointer-returning function; the `consumer` package accepts the imported struct by pointer and reads its field, and constructs/returns the imported enum by pointer; the application entrypoint calls both packages and matches the dependency enum payload.
- The test checks package-scoped LLVM definitions, cross-package pointer signatures, struct field addressing, enum payload allocation, enum discriminant matching, and package-interface records for imported nominal parameter/return types.
- This is compiler-pipeline regression coverage for emitted package IR and entrypoint IR. It is not yet a successfully executed native multi-package program: the environment has no Rust toolchain, so the test could not be compiled/run here. Foundry's existing pre-link layout validator remains the gate that checks the generated package units against their published struct/enum payload layouts before native linking.

## Phase 3 continuation — successful-build state commit ordering (2026-11-11)

- Hardened package artifact persistence so Foundry validates and serializes the complete active package artifact/interface set before publishing any package record. Records are staged to temporary files and only renamed into the cache after all active package artifacts pass validation.
- Reordered successful package-state publication: package artifact records are committed first, graph state second, and package fingerprints last. The executable build fingerprint stamp is now written only after package state persistence succeeds, including the remote-cache restore path.
- Added a regression test with two packages where the first interface is valid and the second is malformed. The test asserts that the failed commit publishes neither a partial package record nor package fingerprint/graph state.
- This narrows the window in which a failed build could leave cache metadata that appears successful. Filesystem failures during the final sequence of multiple atomic renames are still not a fully atomic multi-file transaction; the executable stamp remains the final success marker and should be treated as authoritative.
- Rust tests were added but not executed because this environment does not provide `cargo`, `rustc`, or `rustfmt`.


## Phase 3 continuation — reproducibility, cache isolation, and interrupted-publication recovery (2026-11-12)

- Added a pair of independently rooted but byte-identical two-package projects using the same compiler identity. Their dependency-aware package keys match, and the fake compiler produces byte-identical package LLVM and interface artifacts. Their cache stores remain physically separate; corrupting one project's artifact does not invalidate or trigger recompilation in the other project's warm cache. This is scoped reproducibility coverage for identical inputs/compiler identity, not a claim of real-compiler bit-for-bit reproducibility across toolchains or hosts.
- Added concurrent `build_inner` invocations for two distinct `--target` output directories. Both complete with independent executable stamps, package fingerprint state, graph state, and package artifact stores. This covers concurrency across isolated target directories; concurrent writers to the exact same output directory remain outside this test.
- Added Debug/Release isolation coverage. The profile participates in package fingerprints, and each profile maintains separate artifacts, package state, and executable success stamps.
- Added an interrupted package-record publication scenario by placing a filesystem object at a deterministic staged-record path. The build must fail without changing the last known-good executable, build stamp, package fingerprint state, or graph snapshot. Removing the stale object allows the next build to recover and publish successfully.
- Validation in this environment: ZIP integrity, both Cargo manifest TOML parses, and shell syntax checks for the fake compiler fixture. Rust compilation/tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. All newly added integration scenarios use the production Foundry orchestration with a deterministic fake compiler/linker; real Fusion compiler/native runtime verification remains outstanding.


## Phase 3 continuation — compiler identity and build provenance (2026-11-12)

- Package fingerprints now include the resolved compiler path, SHA-256 of the compiler executable bytes, compiler `--version` output and exit status, target OS/architecture, profile, package inputs, and transitive dependency keys. The package fingerprint domain was advanced to `foundry-package-key-v2`; the root build fingerprint domain is `foundry-build-key-v2`. Replacing a compiler executable in place now invalidates package artifacts even if its path and reported version are unchanged.
- The Fusion CLI now supports `--version`/`-V`/`version`, reporting the Cargo package version. This makes compiler version discovery explicit and machine-readable for Foundry.
- Successful local and remote-restored builds write `.foundry-build-provenance.toml`, recording provenance format version, compiler path/hash/version, package interface version, profile, package fingerprints, package LLVM artifact hashes, and final executable hash. The executable success stamp is still written last.
- Fresh-build reuse now requires valid provenance whose executable SHA-256 matches the current executable; a missing, malformed, or mismatched provenance file forces a rebuild/relink rather than trusting the stamp alone.
- Added regression coverage for compiler binary replacement at a stable path/version, provenance contents, and tampered provenance forcing a fresh link.
- Validation in this environment: archive integrity, Cargo manifest TOML parsing, source-level assertions, and fake compiler shell syntax checks. Rust compilation/tests and a real Fusion-generated native executable remain unverified because `cargo` and `rustc` are unavailable. Compiler executable hashes cover the Fusion binary itself; system linker/Clang identity and dynamically loaded toolchain dependencies are not yet included in the provenance contract.

- Provenance also records the root build fingerprint and the entrypoint LLVM artifact hash; fresh-build reuse requires the stored provenance build fingerprint to match the current expected key.


## Phase 3 continuation — linker identity, ordered LLVM inputs, and remote provenance (2026-11-12)

- Foundry now resolves Clang using the same precedence as Fusion's `link-llvm` implementation (`FUSION_LLVM_DIR`, `LLVM_HOME`, `PATH`, then the supported Windows locations). The resolved linker path, executable SHA-256, `--version` output, and version-command status participate in the root executable fingerprint. Linker changes therefore force a relink without unnecessarily invalidating package LLVM artifacts. The compiler and linker are distinct provenance identities: Foundry invokes the Fusion compiler's `link-llvm` subcommand, which in turn invokes Clang.
- Provenance format advanced to v2. It records linker path/hash/version, the ordered link input list (entrypoint first, followed by package LLVM modules in dependency-first order), each input's SHA-256, and a digest over that ordered list.
- Local fresh-build reuse now compares the complete expected provenance document against the stored document, not only the executable hash. Altered linker identity, input order, or input hashes force a fresh link.
- Remote cache publication now uploads a `.provenance.toml` sidecar before publishing the executable object. Restoration requires both objects and compares the downloaded sidecar semantically against the locally expected compiler/linker identities, package/interface fingerprints, ordered LLVM input hashes, and downloaded executable hash. Missing, malformed, or mismatched sidecars are treated as remote cache misses; unverified executables are discarded and the local link path proceeds.
- Added regression coverage for linker/input provenance comparison and for tampered linker provenance forcing a relink. Existing provenance assertions now expect format v2.
- Validation limitation: Rust compilation/tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. Clang is present at `/usr/local/swift/usr/bin/clang`, but the actual Rust-built Fusion compiler and its native end-to-end executable path are still not run in this environment. The linker identity currently fingerprints the Clang executable and version output; dynamically loaded shared libraries, SDK contents, and implicit environment configuration are not independently hashed.


## Phase 3 continuation — linker target/resource provenance and remote-cache HTTP integration (2026-11-12)

- Root build fingerprint domain advanced to `foundry-build-key-v3`. Linker identity now includes Clang's reported target triple (`-dumpmachine`), resource-directory path (`-print-resource-dir`), a deterministic SHA-256 over regular files and relative paths beneath that resource directory, and the effective linker flag list (`-O2`). The same fields are included in the provenance document, advanced to format v3. This detects changes to builtin Clang headers/resources even when the Clang executable is unchanged. It does not yet hash the host SDK, system linker libraries, environment variables, or all dynamically loaded dependencies.
- The Fusion `link-llvm` implementation now names its effective `-O2` linker flag in one local constant, with an explicit synchronization comment tying it to Foundry's provenance contract. The existing link invocation behavior is unchanged. Foundry's `--target` remains an output/cache namespace selector in this code path; it is not represented as a Clang cross-compilation flag. The actual Clang target triple is recorded separately.
- Added a local TCP HTTP server test harness in `foundry/src/cache.rs`. Integration tests exercise real `curl` PUT/GET operations for the provenance sidecar and executable object, verify their exact stored paths and bytes, and verify that a missing sidecar returns a cache miss and leaves no partial destination file.
- Added a linker identity regression test proving that a Clang resource header change and a reported target-triple change both alter the linker identity fields. Provenance assertions now require target triple, resource directory/hash, and effective linker flags.
- Validation in this environment: source inspection and archive/TOML checks are available; Rust compilation/tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. The local HTTP integration tests are implemented but have not been run here.


### Phase 3 continuation — linker search-path and environment provenance (format v4)

- Extended the Clang identity used by Foundry fingerprints with `-print-search-dirs` output and its digest. This records the compiler driver's discovered program/library search configuration in addition to its binary, version, target triple, and resource directory.
- Captured a bounded allowlist of toolchain-affecting environment variables (SDK selection, Clang config overrides, compiler/library/header search paths, dynamic-library paths, and `SOURCE_DATE_EPOCH`). Set and unset states are both explicit and contribute to the root executable fingerprint.
- Build provenance format advanced from v3 to v4 and now stores the linker search-directory output/hash and the allowlisted environment values. Local freshness validation rejects older provenance formats and compares the complete expected document.
- Added regression assertions for search-directory identity, environment capture, and provenance differences caused by changed search paths or SDK environment values.
- The actual Fusion compiler/native multi-package execution milestone remains unverified: this environment still has no `cargo` or `rustc`, so Rust tests cannot be run here.

### Phase 3 — reproducible real-toolchain integration harness

The next real-compiler milestone is now packaged as an executable developer harness rather than another fake-compiler-only test. `tests/fixtures/phase3-cross-package-foundry` defines a dependency chain `model -> consumer -> app` and exercises cross-package `Point` and payload-bearing `Outcome` values. Run `bash scripts/phase3-real-toolchain-smoke.sh` from any working directory to build Fusion and Foundry, run the compiler's native aggregate ABI test and Foundry orchestration test, build the fixture through the actual Foundry CLI, execute it, and validate compiler/linker/package/interface/link-input/executable provenance. The expected runtime output is `7`, `9`, `failed`. `PHASE3_KEEP_WORKSPACE=1` preserves the temporary workspace after completion. Requires Cargo/rustc, Clang, Bash, and Python 3.11+. This environment lacks Cargo/rustc, so the harness has only received static/syntax validation here; real end-to-end results remain pending.


### Phase 3 continuation — Debug/Release native profile isolation gate (2026-11-13)

- Extended `scripts/phase3-real-toolchain-smoke.sh` to build the real three-package fixture in both Debug and Release profiles using the real Fusion compiler and Foundry CLI. Both resulting executables must produce the expected struct/enum output (`7`, `9`, `failed`).
- The existing Debug executable and its provenance sidecar are hashed before the Release build and checked afterward. A Release build that mutates Debug outputs now fails the smoke gate.
- Release provenance is parsed independently and checked for format v4, the `release` profile, compiler/executable hashes, linker identity, ordered link-input digest, and package interface version 6. A second Release build must preserve executable bytes, complementing the existing Debug warm-build check.
- Updated `docs/phase3-ci.md` so the CI contract and retained evidence explicitly cover both profiles. The existing required GitHub Actions job automatically exercises this stronger script; no separate workflow job or duplicate build is needed.
- Validation in this environment: Bash syntax and Python verifier syntax checks pass; Cargo manifests and fixture manifests parse as TOML; workflow YAML parses when PyYAML is available. The actual Rust compiler/Foundry builds and native Debug/Release executions remain pending because this environment has no `cargo` or `rustc`.


### Phase 3 continuation — dependency lockfile v3 and lock provenance (2026-11-13)

- Upgraded `foundry.lock` to format v3. Every package record now carries an explicit `registry` origin in addition to its source kind, requested requirement, resolved version, revision, and checksum. Registry origin is normalized by removing trailing slashes and is checked against `FOUNDRY_REGISTRY` before a locked registry package is reused. A registry change therefore requires an explicit `foundry update` instead of silently selecting same-named content from another source.
- Path dependencies now require a matching lock entry in offline mode. Their canonical path and deterministic source-tree digest are checked against the lockfile; changed path contents or moved path roots fail with an actionable `foundry update` diagnostic. Git dependencies continue to require immutable commit pins offline.
- A manifest hash mismatch is now an explicit lock drift error. Normal builds no longer silently discard the old lock and resolve a different graph; `foundry update` is the explicit operation that refreshes dependency pins. Legacy lockfile formats are rejected with the same update guidance.
- Path dependency source-tree digests deliberately exclude generated `foundry.lock` and `target` state. This prevents recursive invalidation when dependency packages receive their own lockfiles during dependency-first resolution; each package lockfile is independently included in that package's build fingerprint and provenance.
- Lockfile writes now use unique sibling staging files and atomic rename publication. A failed publication cleans its temporary file and leaves the previous lockfile destination intact. The helper is shared with other Foundry state/provenance writes.
- Workspace registry pin checks now group by registry origin plus dependency alias, so identical aliases from distinct registries are not conflated while packages from the same registry still receive workspace-wide version consistency checks.
- Build provenance advanced to format v5 and records a SHA-256 for every package lockfile (or a stable no-lockfile sentinel). The real-toolchain smoke verifier checks those digests and requires lockfile format v3. Package/root fingerprints already included lockfile bytes; the new provenance fields make that dependency-resolution input independently auditable.
- Added regression tests for stale manifest rejection, path dependency drift, missing path pins in offline mode, registry-origin persistence and drift, atomic lockfile publication failure, and package-lockfile provenance hashes.
- Validation in this environment: source checks, shell syntax, Python verifier syntax, TOML parsing, and ZIP integrity. Rust compilation and tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. The real native integration harness remains pending a Rust-equipped CI run.


### Phase 3 integration-readiness audit (2026-11-13)

- Audited the required GitHub Actions gate and real-toolchain smoke script against the current format-v5 provenance and format-v3 lockfile contracts.
- Expanded Release provenance checks to match Debug coverage for compiler/linker identities, fingerprint stamp, package artifact/interface records, lockfile hashes, dependency-first ordered link inputs, and ordered-input digest. The Debug/Release isolation checks remain in place.
- Corrected stale fixture documentation that referred to provenance format v4.
- Added a release acceptance checklist distinguishing implemented harness coverage from results that require a Rust-equipped CI run and repository branch-protection configuration.
- No CI pass is claimed. Rust toolchain binaries are unavailable in this environment, so Rust formatting, tests, builds, and native execution still require the configured GitHub Actions run.
