Foundry - Fusion project/package/toolchain manager

The package manager for Fusion projects. It provides commands to create, build, run, and manage Fusion packages and their dependencies.
It is responsible managing dependency resolution, building Fusion programs, running and testing them, compatibility checks, publishing and more.
It resolves version, dependencies and compatibility issues for plugins.

Bazel, in particular, already supports multi-language build orchestration, so Foundry should not rely on multi-language builds alone as its novelty.
We should relly on Bazel(a multi-language build system) and Cargo(a package manager with dependencies and builds + reproducability with cargo.lock) for guidance, knowledge and inspiration.

Foundry: Supplies dependency resolution, build tooling, test execution, compatibility management and publishing.


I would divide Foundry into four responsibilities, each with a clearly defined boundary.

1. Package manager

Download and resolve dependencies.
Manage package versions and a lockfile.
Support local, Git, and registry dependencies.
Handle optional dependencies and features.
Build and publish Fusion packages.
Manage workspaces.
This should be the foundation of Foundry, with predictable and reproducible dependency resolution.

2. Build orchestrator

This is where Foundry becomes different from a conventional language package manager.
Determine which compilers are required.
Build Fusion and foreign-language source files.
Manage compilation order and build dependencies.
Coordinate linking and artifact generation.
Cache build outputs.
Support cross-compilation and build profiles.
Foundry should delegate compilation to the appropriate compiler instead of implementing the build logic of every language itself.

3. Toolchain and plugin manager

This is potentially the most distinctive part.
Detect and provision compatible compilers, SDKs, and runtimes.
Install and update integration plugins.
Check plugin and toolchain compatibility.
Configure environment variables and compiler flags.
Generate or manage language bindings.
Handle foreign-language integration strategies.
For example, a C plugin could define how to compile C files, expose native libraries to Fusion, and link the resulting artifacts.
Foundry could download or provision a C toolchain where licensing and platform constraints permit, rather than assuming the user has installed it.

4. Project workflow

This is the developer-facing experience.
foundry new my_project
foundry add serde
foundry build
foundry run
foundry test
foundry doc
foundry publish

Developers should not need to know which internal tool performs each operation. Foundry provides the unified interface while still allowing access to lower-level commands when needed.

One important architectural decision is to separate packages, plugins, and toolchains. They are related, but they should not be the same thing.



We will build foundry in stages:
Stage 1 — Minimal project manager

Implement:
foundry new
foundry build
foundry run
foundry test
Basic fusion.toml
Local package dependencies
The objective is to have one working Fusion project that can be built consistently.

Stage 2 — Dependency management
Add:
Version constraints
Dependency resolution
fusion.lock (or a similarly named lockfile)
Git and registry dependencies
Basic workspaces
At this point, Foundry starts to resemble Cargo in its core package-management responsibilities.

Stage 3 — C integration First interoperability proof
Build a minimal C plugin that:
Identifies required C source files.
Invokes a compatible C compiler.
Produces native object files or libraries.
Links them with Fusion's output.
Exposes C functions to Fusion through explicit declarations and bindings.
Use a small real C library to demonstrate that a Fusion programmer can consume foreign code without manually orchestrating the C build.

Stage 4 — Plugin and toolchain management
Add plugin versioning, compatibility checks, toolchain provisioning, caching, and a documented plugin interface.
Keep plugin execution controlled and secure. Plugins that can run arbitrary build commands should be treated as executable code, not harmless configuration.

Stage 5 — More ecosystems
Add further integrations for third-party created plugins.


Foundry should orchestrate, not reinvent.
Use existing, mature infrastructure whenever possible:
Rust's compiler and Cargo.
LLVM for native code generation.
Existing C and C++ compilers and linkers.
Existing managed runtimes.
Existing registries and build systems where appropriate.


When you install fusion(vanilla/basic), you get something like this:
Fusion installation(folders)
│
├── fusion compiler/interpreter
├── docs(dcumentation)
├── fusion package manager(foundry)
├── (if an ecosystem plugin was downloaded with foundry) the fusion toolchain
├── (if an ecosystem plugin was downloaded with foundry) static analyser (suggests moving fusion code into another programming language)
├── (if an ecosystem plugin was downloaded with foundry) interop model (handles the 5 levels of integration)
└── (potential) plugins (downloaded with foundry)
       │
       ├── C plugin (also downloads the toolchain, static analyser, interop model. Ecosystem plugins require them)
       ├── C++ plugin (also downloads the toolchain, static analyser, interop model. Ecosystem plugins require them)
       ├── Rust plugin (also downloads the toolchain, static analyser, interop model. Ecosystem plugins require them)
       ├── SQLite plugin (does not download the toolchain, static analyser, interop model. Ergonomic plugins don't need them)
       ├── Cloud-build plugin (does not download the toolchain, static analyser, interop model. Ergonomic plugins don't need them)
       └── ...


Currently we have all plugins(folders) available, even if many are underdeveloped!
Foundry only downloads + installs fusion's toolchain, static analyser, interop model if an ecosystem plugin is required, this way the initial installation is small.


Running foundry new hello_world should create a real, usable project with a familiar structure.
hello_world/
src/
Application source code, starting with main.fusion.
tests/
Integration tests and test programs.
examples/
Runnable example programs.
Foundry.toml
Project configuration.
foundry.lock
Reproducible dependency resolution.
.gitignore

Cargo-inspired commands
These should be first-class Foundry commands, not separate scripts.
Command	Expected behavior
foundry new hello_world	Create a new project
foundry init	Initialize the current directory
foundry check	Validate source and types without producing a release executable
foundry build	Build the project
foundry run	Build and execute the application
foundry test	Discover, compile, and execute tests
foundry fmt	Format Fusion source
foundry clippy	Run Fusion's static analyzer and lint checks
foundry add <dependency>	Add a dependency to the manifest
foundry remove <dependency>	Remove a dependency
foundry update	Update dependencies within their allowed constraints
foundry clean	Remove generated build artifacts
foundry doc	Generate documentation
foundry tree	Display the dependency graph
foundry metadata	Print machine-readable project metadata
The crucial distinction is that foundry test must actually execute tests and report failures. Merely checking that test files compile is not sufficient.



Add Bazel-inspired build capabilities

- Correct dependency graphs: build only the targets affected by a change.
- Incremental builds: fingerprint source files, compiler versions, flags, target platform, and dependencies.
- Parallel execution: compile independent modules and targets concurrently.
- Reproducible builds: lock dependency revisions and track the toolchain used.
- Multiple targets: applications, libraries, tests, examples, and eventually foreign-language artifacts.
- Extensible toolchains: let plugins contribute build actions without embedding every language's rules into Foundry.
- Optional remote caching: a later capability, once local caching and fingerprints are reliable.
Foundry shouldn't reinvent LLVM, C compilers, or existing language build systems. It should orchestrate them through a consistent build model.


A Fusion developer shouldn't need to learn a different command set for every language.

Remember there is a folder called "toolchain-orchestrator", "fusion-interop-model", "static analyser" inside of the "Shared ecosystem infrastructure" folder that is downloaded, when the user needs an ecosystem plugin. The responsibilities must not overlap with foundry.


Foundry owns the standard Fusion development workflow. Optional ecosystem components may extend that workflow through defined interfaces, but they must not duplicate Foundry's core responsibilities. Optional components are acquired and initialized only when required by an explicitly selected capability or plugin.


-------------------------------------------------------------------------------------------------------------------------------------------------------------



What's been added

Dependency commands
foundry add and foundry remove now edit the project manifest, supporting version, path, Git, and development-dependency declarations.

Incremental build caching
Builds fingerprint Fusion source contents, manifest configuration, compiler path and version output, target platform, profile, and compiler arguments. A matching existing build can be reused.

Test execution
foundry test now executes each .fusion test file with fusion run and reports failures, instead of merely checking whether files compile.

Additional developer commands
Added foundry fmt, foundry clippy, and foundry doc. Formatting and analysis delegate to the Fusion compiler, while documentation generates a basic package overview.

Manifest compatibility
Found and addressed the mismatch between Foundry.toml in your project template and fusion.toml expected by the implementation. Foundry.toml is now preferred, while existing fusion.toml projects remain supported.



What remains for the next iteration
Some parts also need further work before they are production-ready:
- Transitive dependency resolution and passing downloaded packages into the Fusion compiler are not yet fully integrated.
- The registry does not yet implement publishing, authentication, or full SemVer prerelease ordering.
- The remote cache still needs authentication and artifact-digest verification.
- Multi-target builds rely on the compiler supporting the requested targets.
- The formatter and analyser depend on fusion fmt and fusion analyze being available.

## Improvements in this iteration

### `foundry doctor`

Run `foundry doctor` from a project directory to inspect the current package configuration.
It reports source/test layout, checks whether the selected Fusion compiler can be executed,
flags missing path dependencies, and points out when registry configuration may be required.
Warnings about optional setup do not fail the command; blocking checks return a non-zero exit code.

### Manifest validation

Foundry now parses `Foundry.toml` and legacy `fusion.toml` as real TOML rather than splitting
each line manually. This supports escaped strings and inline dependency tables reliably, and
validates dependency source declarations (`version`, `path`, or `git`) before commands proceed.
Dependency names are restricted to letters, digits, `_`, and `-` to prevent unsafe names from
being used as cache paths.

### Safer workflow behavior

- `foundry metadata` JSON-escapes names, versions, and paths so its output remains valid JSON.
- `FOUNDRY_OFFLINE` is respected during builds and multi-target builds as well as dependency fetches.
- `foundry clean` removes debug/release/doc outputs while preserving the downloaded dependency and
  toolchain cache under `target/foundry`.
- Relative `FUSION_COMPILER` paths are resolved before Foundry changes the child process directory.

### Verification note

The source archive was updated with tests for TOML manifest edge cases, JSON escaping, and cache
preservation. This environment did not have `cargo`, `rustc`, or `rustfmt` installed, so the Rust
test suite could not be executed here; run `cargo test` in a Rust development environment before
merging this iteration into a release.

## Package dependency graph

`foundry::package_graph::resolve_package_graph` builds a deterministic graph of the current package and its resolved dependencies. It distinguishes package roots from the Fusion compiler's per-package module graph, records dependency aliases, and returns dependency-first build order. Path dependencies are traversed directly; Git and registry dependencies are read from `target/foundry/deps/<dependency-alias>` after dependency resolution. The build fingerprint includes Fusion sources from every package in this graph.

This is the package-discovery foundation, not yet cross-package compiler import support. Compiler module search roots and package-qualified import semantics remain a separate integration milestone.

### Reproducible dependency resolution

`foundry.lock` pins registry dependencies to exact versions and archive SHA-256 checksums, and Git dependencies to resolved commit IDs. A normal build reuses those pins; it does not advance a branch or tag just because the remote moved. Run `foundry update` to fetch current registry/Git metadata and deliberately refresh the selected versions/commits. `foundry fetch --offline` requires dependencies to be cached and pinned in the lockfile; it does not resolve an unpinned Git selector or registry requirement from a cached moving version index. Exact registry pins use the lockfile version directly and do not require the versions index to be cached. `foundry update --offline` (or `FOUNDRY_OFFLINE=1 foundry update`) is rejected because an update must consult current remote metadata.

Dependency-free projects may have a header-only lockfile. Lockfile records are checked for required fields, supported source kinds, exact registry versions/checksums, duplicate aliases, and aliases no longer declared by the manifest. A single dependency alias cannot be declared in more than one of `[dependencies]`, `[dev-dependencies]`, or `[plugins]`, because Foundry's lockfile and package graph identify dependencies by alias. Git branch selectors are resolved through fetched remote-tracking refs, while an omitted selector uses the remote's advertised default branch; both are converted to immutable commit IDs before checkout. SHA-1 and SHA-256 Git object IDs are accepted.


### Per-package LLVM artifact cache

Foundry emits each resolved package independently through the Fusion compiler's `emit-package-llvm` command, in dependency-first order, and stores the resulting package-scoped LLVM module in the profile/target package-artifact store. Cache records bind the package fingerprint, artifact kind, interface path, and SHA-256 digests; corrupted or incompatible outputs are rebuilt. The final application links the cached package modules with a separate entrypoint/shared-runtime module. Link-time checks compare package function signatures and scalar data globals against the package interface before invoking Clang.


### Package-owned scalar data linking (Phase 3 continuation)

Package interfaces are now version 4. Public and private scalar literal constants receive deterministic module-qualified LLVM data symbols; public constants use `abi-data` records and package-private constants use `abi-internal-data` records. The LLVM backend emits each supported constant as a package-owned `constant` global, and package functions load from those globals by stable symbol. String constants use private literal byte storage plus a pointer-valued constant global.

Package LLVM units retain their own data definitions and emit `external constant` declarations only for referenced globals owned by other modules. The application entrypoint similarly declares referenced package globals rather than defining duplicate copies. Foundry validates data symbol ownership, provider uniqueness, scalar LLVM types, public/private visibility, and root-entrypoint access to root-private constants before linking. Package artifact records use `llvm-package-ir-v4`, forcing older cache entries to be regenerated.

Supported initial data ABI: `i64`, `double`, `i1`, and `ptr` scalar literals. Non-literal constants and aggregate/static data are not yet linkable across package boundaries.

## Phase 3 dependency lockfile contract

Foundry's current `foundry.lock` format is version 3. It binds the lockfile to the exact manifest bytes and records each dependency alias, requested constraint, resolved version or Git commit, source identity, registry origin where applicable, and integrity digest. Registry archives are pinned by SHA-256; local path dependencies are pinned by canonical path and a deterministic source-tree digest; Git dependencies are pinned by immutable commit ID.

Normal builds reject a stale manifest, a changed path dependency, or a changed registry origin and direct the developer to run `foundry update`. Offline builds require existing lock entries for path and Git dependencies, and exact version/checksum pins plus cached verified registry metadata and archives for registry dependencies. Lockfile updates are staged and published with a unique temporary sibling file so failed writes do not leave a partial lockfile.

Build provenance format 5 records a SHA-256 for every package lockfile. The package and root build fingerprints also include lockfile bytes, so changing dependency resolution invalidates the affected cached build artifacts.

### Registry extracted-cache integrity

Registry archives are checksum-verified before extraction. Foundry separately fingerprints each extracted package tree and stores a local tree-manifest record bound to the archive checksum. Every resolution checks the extracted tree; modified or incomplete cache directories are restored from the verified archive, including offline. Archive downloads and extracted-directory replacement are staged so failed transfers/extractions do not publish partial cache objects. Archive preflight rejects traversal paths, absolute paths, symlinks, hard links, and special files; see [`docs/registry-cache-integrity.md`](docs/registry-cache-integrity.md) for the integrity model and archive restrictions. The tree manifest detects ordinary local cache corruption but is not a signature against an attacker able to modify both the package directory and its manifest.

Registry cache writers are serialized by a per-package process lock, stale locks are reclaimed only when the recorded process is confirmed gone, and interrupted extraction siblings are cleaned after a verified package is available. The cache is workspace-local under `target/foundry/deps`; `foundry cache prune` removes registry versions not referenced by that workspace's validated `foundry.lock` and leaves other workspaces untouched.


### Registry cache diagnostics

Use `foundry cache inspect` for a read-only report of cached registry package metadata, archive checksums, extracted directories, and extracted-tree integrity sidecars. `foundry cache inspect --json` emits the same per-package statuses as JSON for scripts. Typical statuses include `healthy`, `archive-missing`, `archive-corrupt`, `extracted-missing`, `tree-corrupt`, and `tree-manifest-missing-or-invalid`. Inspection does not repair or delete entries; run `foundry fetch` to re-verify and repair an extracted-tree mismatch. If the archive itself is corrupt, the resolver removes it and reports the checksum failure; run `foundry fetch` again to download a fresh copy. Use `foundry cache prune` to remove unreferenced versions.


### Registry protocol hardening

Registry package metadata now requires `format_version = 1` and exact package identity fields. Remote registry and archive traffic is HTTPS-only (including redirects); local absolute paths and `file://` locations remain available for local registries. URL credentials, fragments, unsupported schemes, and relative archive locations are rejected. Registry index and metadata cache records are written through staged atomic publication, and malformed metadata is rejected before it can replace a previously cached valid record. See `docs/registry-protocol.md` for the format contract and compatibility rules.
