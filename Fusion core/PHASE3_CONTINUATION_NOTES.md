# Phase 3 continuation — module source-root isolation

## Implemented

- Module resolution now canonicalizes a candidate before accepting it.
- A resolved module must remain inside the entry project's source root.
- The check also catches symlink escapes, not just textual `..` traversal.
- The diagnostic identifies the requested module, importing file, and resolved path.
- Added a Unix regression test that imports a symlink whose target is outside the project root.

## Why

A package's module graph must not silently absorb arbitrary source files outside its source tree. This makes module identity and package boundaries more predictable and gives Foundry a safer foundation for package-level dependency integration.

## Validation status

The Rust toolchain is not installed in this execution environment (`rustc` and `cargo` are unavailable), so the test suite could not be compiled or executed here. The regression test is included but remains unverified at runtime.

### Latest Phase 3 continuation — Foundry package graph foundation

- Added `foundry::package_graph`, a deterministic package-level dependency graph that is deliberately separate from the compiler's source-module graph.
- Package nodes retain canonical package roots, package names/versions, canonical `src` roots, and a conventional or first discovered source entry point.
- Package edges retain the dependency alias from the parent manifest and the canonical dependency package root.
- Path dependencies are traversed recursively. Git and registry dependencies are read from Foundry's populated `target/foundry/deps/<alias>` cache after dependency resolution.
- Package cycles are rejected with a readable canonical-path chain. Missing cached dependencies and missing `src/` roots receive explicit errors.
- Foundry's build path now constructs this graph before invoking Fusion and includes every package's Fusion source files in the build fingerprint. A change to a transitive path dependency therefore invalidates the application build cache.
- Added tests for a three-package path-dependency chain, deterministic dependency-first ordering, library package entry discovery, and package-cycle diagnostics.
- Important boundary: this increment does **not** yet make compiler imports resolve across package roots. It establishes Foundry's package graph and cache correctness first. The next compiler-side step is to pass an explicit package/module search-root description into `load_project_with_metadata`, then map package-qualified imports to compiler module identities without allowing dependencies to escape their own package roots.
- Validation limitation: `cargo`, `rustc`, and `rustfmt` are unavailable in this environment, so the Rust implementation and tests could not be compiled or executed. The new tests are source additions, not verified passing tests.


## Cross-package import bridge (current increment)

- Added `module_system::load_project_with_package_roots` and `compile_project_with_package_roots` so the compiler receives an explicit map of Foundry dependency aliases to canonical package `src/` roots.
- Added compiler CLI support for repeated `--package-root alias=src-path` arguments.
- Foundry now passes only the root package's direct dependencies to the compiler. This intentionally keeps transitive dependencies private unless the package exposes them through its own public module API.
- `import math` resolves a dependency root through `src/lib.fusion`, `src/main.fusion`, or `src/mod.fusion`; `import math.geometry` resolves `src/geometry.fusion` or `src/geometry/mod.fusion`.
- Dependency module identities are package-qualified (`math`, `math.geometry`), so existing visibility checks and namespace lowering apply to imported dependency declarations.
- Imports made inside a dependency resolve relative to that dependency's own source root. Canonical-path checks prevent symlinks from escaping the selected package source root.
- Added regression coverage for direct dependency imports, dependency-local module imports, and public-visibility enforcement.

### Remaining Phase 3 work

- Re-exported dependency APIs across package boundaries need dedicated end-to-end tests and validation.
- Nested dependency visibility and package-qualified type/trait/constant resolution need broader coverage.
- Foundry lockfile resolution now pins registry versions and Git commits and records path source fingerprints; workspace-wide version conflict handling and incremental package-level compilation remain separate follow-up tasks.
- Manifest dependency aliases containing hyphens are normalized to underscores for Fusion import syntax (for example, `my-math` becomes `my_math`).

### Validation

- ZIP integrity was checked after packaging.
- The environment does not provide `cargo`, `rustc`, or `rustfmt`; therefore Rust compilation and tests could not be executed here. The new tests are included but are not claimed to pass until run in a Rust toolchain environment.

## Re-export correctness increment — selective and dependency package re-exports

- Re-export propagation now resolves each edge through the same module-aware target resolver used by semantic lookup, rather than assuming an import path is already a globally qualified module identity. This allows a dependency package root such as `facade` to re-export declarations from its own `facade.geometry` module.
- `pub from module import Name` is now recorded as a public selective re-export. Only the explicitly selected names are propagated; an ordinary `pub import module` continues to re-export the target module's full public surface.
- Added regression tests for selective re-export filtering and for a dependency package re-exporting a type from an internal module, including qualified HIR definition resolution back to the physical declaring module.
- Validation: source changes and regression tests were added. This environment has no `cargo`, `rustc`, or `rustfmt` executable on `PATH`, so compilation and test execution remain unverified. ZIP integrity is checked separately when packaging.

## Transitive dependency resolution and registry alias materialization

- Foundry now resolves dependency manifests recursively, not just the root manifest. Runtime dependencies of each package are resolved in that package's own cache and lockfile; only the root package additionally resolves dev-dependencies and plugins.
- Recursive resolution tracks canonical package roots and reports dependency cycles with a readable package path chain.
- Registry archives are extracted into their versioned cache directory by the registry client, while the package graph expects dependencies under the manifest alias. Foundry now materializes the extracted package at that alias path, including renamed dependencies, so package graph traversal can find it consistently.
- Alias materialization copies regular files and directories, rejects directory symlinks, and rejects symlinks that resolve outside the extracted package root.
- Added regression tests for transitive path-dependency lockfile creation and registry package alias materialization.
- Validation status: source-level checks only. No Rust toolchain executable (`cargo`, `rustc`, or `rustfmt`) is available in this environment, so compilation and test execution could not be performed.

## Lockfile validation and immutable Git revisions

- Foundry now parses lockfile v2 entries into structured records and rejects malformed entries, missing required fields, unsupported lockfile versions, and duplicate dependency aliases.
- A lock entry is accepted only when its source and requested dependency selector/requirement still match the manifest. Registry versions are pinned to the exact locked semantic version and the archive checksum is checked against the lock entry.
- Provisional `unresolved` registry records written by `foundry add` remain valid as pre-resolution placeholders; online resolution replaces them with an exact version. Offline resolution refuses an unpinned registry entry.
- Git dependencies are written with the resolved full commit ID. Normal builds check out that immutable commit and do not fetch moving branches/tags. `foundry update` fetches remote refs and resolves the requested selector again, then records the newly selected commit.
- Offline Git resolution requires a cached repository and a full SHA-1 or SHA-256 commit ID in the lockfile; it will not silently follow a branch or tag.
- Added unit coverage for lockfile record validation, duplicate aliases, stale manifest hashes, and Git object-ID validation.
- Validation remains source-level: this environment has no `cargo`, `rustc`, or `rustfmt` executable available, so Rust tests could not be run here.


## Lockfile correctness follow-up — empty projects and Git selector updates

- Header-only version-2 lockfiles are now valid for packages with no dependencies; the parser distinguishes an absent `[[package]]` array from a malformed non-array value.
- Lock entries validate supported source kinds and source-specific fields. Registry pins require an exact version plus a 64-character SHA-256 archive checksum; path pins require a directory fingerprint; Git entries cannot carry registry checksums. Dependency packages write only runtime dependency records to their own lockfiles; their dev-dependencies/plugins are not accidentally emitted as stale runtime entries.
- Stale lock aliases are rejected, and dependency aliases may not be declared in multiple manifest sections because the current lockfile/package graph keys records by alias.
- Offline registry resolution now requires an exact lock entry even when no lockfile exists; it cannot select a new version merely from a cached range index. A pinned exact registry version bypasses the moving versions index and needs only its cached metadata/archive. `foundry fetch` and `foundry update` honor both `--offline` and `FOUNDRY_OFFLINE`; updates are rejected in offline mode.
- Git selectors now resolve branch names through `refs/remotes/origin/*` when no local branch exists, resolve tags explicitly, and use the remote's advertised default branch when no selector is specified. The selected ref is converted to a full object ID before detached checkout, so `foundry update` actually advances an unqualified Git dependency. Both SHA-1 and SHA-256 Git object IDs are accepted.
- Added regression tests for empty lockfiles, malformed/duplicate/stale entries, duplicate manifest aliases, offline registry pins, offline updates, SHA-1/SHA-256 IDs, and a local bare-Git remote with a non-default branch.
- Validation limitation: this environment has Git but no `cargo`, `rustc`, or `rustfmt`. The local Git-ref resolution scenario was reproduced successfully with shell Git commands; Rust compilation, formatting, and test execution remain unverified.


## Package-qualified trait/type re-export regression coverage

- Added a package-root integration regression for a dependency facade that selectively re-exports a trait from an internal contracts module and a struct from an internal models module.
- The test resolves both names through the consumer import alias and asserts that HIR identities point to their physical declaration modules, not the facade module.
- The same test verifies that unselected traits and private types remain inaccessible through the facade, protecting the public API boundary.
- This increment adds regression coverage for functionality already present in module-aware namespace lowering and HIR export-provider traversal; it does not claim to close the broader package-qualified type/trait work.
- Validation limitation: `cargo`, `rustc`, and `rustfmt` are unavailable in this environment. The test has been added but cannot be compiled or executed here.


## Package-qualified trait dispatch and multi-level re-export continuation

- The parser now accepts dotted trait and receiver names in `impl Trait for Type` declarations (for example, `impl facade.Readable for facade.Point`). Previously `parse_impl` consumed only one identifier for each name, even though ordinary type annotations already accepted dotted module-qualified names.
- Added a package-root regression scenario with a three-step public API chain: `facade` re-exports `api`, `api` selectively re-exports `Readable` and `Point`, and the declarations live in separate `contracts` and `models` modules.
- The regression resolves both public declarations to their physical HIR owners, rejects unselected/private declarations, and resolves the consumer's package-qualified trait method to an identity scoped by the original trait and receiver types.
- This closes the parser-side gap for package-qualified trait implementations and adds end-to-end module-graph/HIR method-resolution coverage. It does not yet claim that every runtime backend dispatch path is HIR-native; interpreter/type-checker method lookup still has compatibility-AST keyed paths that need migration.
- Incremental compilation is not yet package-granular: Foundry currently computes a deterministic whole-build fingerprint that includes dependency sources. A true package cache needs stable package build artifacts/IR and dependency-keyed cache entries, not only per-package timestamps.
- Lockfile-aware conflict resolution remains distinct from exact lockfile pin validation. Current lockfiles validate aliases, source selectors, exact registry pins/checksums and immutable Git commits; the next step is workspace/package-graph-level constraint unification and explicit incompatible-version diagnostics.
- Validation: source edits and regression tests were added. `cargo`, `rustc`, and `rustfmt` are unavailable in this environment, so Rust compilation, formatting and test execution could not be performed. ZIP integrity can be checked, but the new test is not claimed to pass until run with a Rust toolchain.

## Package-qualified trait method call-site regression

- Extended the three-level selective re-export fixture so the consuming package declares a trait implementation using package-qualified trait and receiver names, then calls that method from a function whose parameter is typed through the facade alias.
- The regression now checks `resolve_hir_calls`, asserts that the method call resolves to the same stable HIR declaration identity as direct method lookup, and verifies that its receiver identity is the original struct declaration in the dependency's internal models module.
- The same fixture invokes `compile_project_with_package_roots` to cover the complete compiler pipeline (parser, package-aware module loader, HIR lowering, nominal/trait validation, and compilation) rather than only symbol lookup.
- This is regression coverage for package-qualified trait dispatch and multi-level re-exports; it does not establish that every runtime backend has completed its migration to HIR-only dispatch.
- Validation limitation: `cargo`, `rustc`, and `rustfmt` are not installed in this environment. The new Rust test could not be compiled or executed; it is source-level coverage only.

## Package-granular build-key foundation

- Added deterministic package-level build fingerprints in dependency-first graph order.
- Each package key includes its package identity/version, manifest bytes, lockfile bytes (or an explicit no-lockfile marker), Fusion source paths and contents, compiler identity/version, build profile, extra compiler arguments, and direct dependency aliases plus their already-computed package keys.
- A transitive dependency change now propagates through the keys of its dependents. This also captures dependency-manifest and lockfile changes that were not represented by the previous source-only traversal.
- The application build fingerprint now incorporates the package keys rather than separately walking dependency source files. This keeps one authoritative package-key calculation and prepares Foundry for per-package compiler artifacts.
- Important limitation: this is package-granular invalidation metadata, not yet package-granular compilation or artifact reuse. The Fusion compiler currently receives the complete application and dependency roots in one invocation, so Foundry cannot safely skip compiling an individual package until the compiler exposes stable package-level IR/artifact outputs.
- Workspace-wide version unification is also not implemented in this increment. Current lockfiles are package-local and key entries by manifest alias; a correct graph-wide resolver needs a central resolution identity that records registry identity + package name independently of the importing alias, and a deliberate policy for allowing multiple incompatible versions side-by-side. Silently forcing a single version would break valid dependency graphs.
- Regression coverage added for propagation of transitive source and package-manifest changes through dependency-first package keys.
- Validation: source inspection and ZIP integrity checks only. This environment does not contain `cargo`, `rustc`, or `rustfmt`, so Rust compilation and tests remain unverified.


## Cached LLVM link integration (November 2026)

- Added the Fusion CLI command `link-llvm <input.ll> [-o output]`, which lowers an existing LLVM IR file through the same Clang toolchain used by `fusion build`.
- Refactored ordinary `fusion build` to use that shared link path after emitting its temporary IR.
- Foundry now links the root package's cached LLVM translation unit instead of invoking a second full `fusion build` compilation after package artifact emission. A cache hit therefore avoids recompiling the root translation unit before final native code generation.
- Package IR files are not concatenated or passed as independent linker modules: the current LLVM backend emits whole-project translation units when package roots are supplied, so doing so would risk duplicate definitions. The dependency package artifacts remain independently cached, while the root unit is the current complete link input.
- This is the first real cache-to-link connection, but not yet true independently linkable package compilation. The next compiler milestone is to emit package-scoped LLVM modules with stable exported/imported symbol interfaces, then link the root and dependency modules together.
- Added a CLI regression test confirming `link-llvm` reports a missing input before attempting to locate Clang. Rust tests could not be run in this environment because `cargo`/`rustc` are unavailable.


## Latest continuation — package-scoped multi-module linking

Foundry's per-package artifact path now uses the package-scoped LLVM emitter rather
than caching a whole-project translation unit for each package. All units are emitted
against the application entry module graph so package-qualified mangled names are
consistent. The application entrypoint and shared runtime are emitted separately;
package functions and runtime helpers become external declarations in the consuming
units. The linker accepts multiple `.ll` inputs, and Foundry links the entrypoint/runtime
module with the dependency-first package module list. Runtime heap/tuple helper state is
owned once by the entrypoint module. Package cache records were upgraded to format v4
and `llvm-package-ir-v2`. Empty units are emitted for unimported dependencies and
main-only roots. Direct package-global data references and unsupported aggregate,
generic, async or inferred ABI signatures remain outstanding. A Clang multi-input smoke
test passed; Rust tests remain unexecuted because the Rust toolchain is unavailable.

## Package-owned data and aggregate layout ABI metadata (November 2026)

- Upgraded package interfaces to v3. Public scalar literal constants now carry deterministic records with package/module ownership, stable mangled data-symbol names, native scalar type, and an unambiguous value encoding (signed integer, IEEE-754 bit pattern, boolean, or UTF-8 hex).
- Public struct declarations now carry a stable mangled type symbol, ordered field records, field count, and a SHA-256 layout digest over the canonical field sequence. Supported scalar field types are represented directly; array and aggregate fields remain explicitly marked unsupported rather than receiving guessed native layouts.
- Foundry validates v3 interface records before accepting fresh or cached package artifacts. It checks record shape, stable symbol syntax, duplicate data/type identities, field counts, field uniqueness, supported field markers, layout digest consistency, and scalar value/type encoding. Invalid interface metadata prevents the package artifact from being treated as a cache hit or committed into the package artifact store.
- Package cache records continue to use format v5 / `llvm-package-ir-v3`; v2 interface bytes are rejected by the v3 header validation and regenerated. The serialized cache-record schema itself did not change.
- Important boundary: this increment establishes package-owned data/type ABI metadata and validation, not native global-data emission. The LLVM backend still lowers `const` declarations through its existing function-local path, and package functions that reference package/global data remain unsupported by package-scoped LLVM emission. The next backend increment must emit package-owned globals, generate matching LLVM global declarations, and connect imported constant references to those symbols. Struct layout metadata is not yet used to emit cross-package aggregate values.
- Added compiler regression coverage for public scalar constants and ordered struct layout records, plus Foundry validation tests for malformed layouts, duplicate constants, and invalid scalar encodings. Rust compilation/tests/formatting remain unverified because this environment has no `cargo`, `rustc`, or `rustfmt` executable.
