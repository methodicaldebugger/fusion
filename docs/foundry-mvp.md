# Foundry MVP: project and toolchain orchestration

**Status:** Proposed design  
**Scope:** First implementation milestone for Foundry  
**Repository baseline:** Existing Fusion CLI supports single-file `run`, `check`, `emit-llvm`, and `build`. `src/foreign.rs` contains an early C-oriented foreign-library model. This document describes planned behavior, not features that are already implemented.

## Goal

Foundry is Fusion's project, dependency, and toolchain orchestrator. It must make a project containing Fusion code and foreign source files build as one project, while keeping the Fusion language implementation independent of individual foreign toolchains.

The first proof is deliberately small:

> A user creates a Fusion project, adds a local C library, and builds/runs the project through Foundry without manually invoking Clang or a linker.

Do not attempt to support every package registry, every plugin, or remote/cloud builds in the first milestone.

## Responsibilities and boundaries

- **Fusion compiler:** parse, resolve names, type-check, interpret, and emit Fusion LLVM IR.
- **Foundry core:** discover the project, parse its manifest, resolve dependencies, validate paths and configuration, select toolchain providers, orchestrate build steps, cache artifacts, and produce actionable diagnostics.
- **C integration plugin/provider:** compile C sources and describe the native link inputs required by the project.
- **Future providers:** C++, Rust, Zig, JVM/.NET, WebAssembly, process/RPC, and cloud builds. Each integration chooses an appropriate boundary; not every ecosystem should be forced through the C ABI.
- **Package sources:** supply versioned package metadata and content. A package source is not itself a compiler plugin.

The core should use a structured process API (`Command` and explicit arguments), never construct a shell command from manifest strings.

## Proposed first user experience

Commands for the MVP:

- `fusion init [path]` — create a project skeleton and manifest.
- `fusion check` — check the project's Fusion entry point.
- `fusion run` — build as needed, then run the project.
- `fusion build` — build the project.
- `fusion add <dependency>` — add a dependency to the manifest.
- `fusion install` — resolve declared dependencies and write/update the lockfile.
- `fusion clean` — remove generated artifacts.
- `fusion toolchain list` and `fusion toolchain doctor` — inspect available providers and diagnose missing tools.

The existing single-file forms (for example, `fusion run src/main.fusion`) should remain usable during migration. If the current command parser makes project and file modes ambiguous, detect a manifest in the current directory and provide an explicit `--manifest-path` escape hatch.

## Proposed project layout

```text
hello-fusion/
├── fusion.toml
├── fusion.lock
├── src/
│   └── main.fusion
├── foreign/
│   ├── mathlib.h
│   └── mathlib.c
└── build/                 # generated; ignored by version control
```

## Proposed manifest

Use TOML for the first manifest format. Keep the schema intentionally small and versioned.

```toml
[package]
name = "hello-fusion"
version = "0.1.0"
edition = "2026"

[build]
entry = "src/main.fusion"

[plugins]
c = "builtin"

[foreign.mathlib]
kind = "c"
sources = ["foreign/mathlib.c"]
include_dirs = ["foreign"]
```

This is a proposed schema, not a promise that this exact syntax is already accepted. Validate unknown keys and unsupported plugin versions with clear errors. Paths must be relative to the manifest directory by default; reject path traversal where a package boundary is expected.

Keep dependency declarations separate from toolchain requirements. For example, a future `[dependencies]` section identifies packages; `[plugins]` identifies build capabilities/providers. A C integration plugin is not the same thing as a C library dependency.

## Lockfile and reproducibility

The lockfile should record resolved dependency identities and immutable content references, not merely repeat loose version constraints. At minimum, design for:

- exact package versions and source identities;
- immutable Git commit IDs or registry checksums where applicable;
- plugin/provider identities and versions when they affect the build;
- target and relevant build options;
- a lockfile schema version.

Do not claim fully reproducible builds until compiler versions, linker versions, SDK inputs, environment-sensitive flags, and relevant native dependencies are accounted for. The MVP can start by locking dependency resolution and recording toolchain versions.

## Build pipeline

1. Find and parse `fusion.toml`.
2. Validate the manifest schema and project-relative paths.
3. Resolve dependencies and verify the lockfile.
4. Select the required toolchain provider; report missing tools before starting the build.
5. Compile foreign sources to object/static-library artifacts in a project-local build directory.
6. Compile/check Fusion source using the existing Fusion pipeline.
7. Link Fusion output with foreign artifacts and native link inputs.
8. Store outputs and build metadata in the build directory.
9. Run the produced program for `fusion run`.

The current `src/foreign.rs` compiles C sources into one object with a direct `Command::new("clang")`. Before Foundry relies on it, consolidate compiler discovery with the CLI's existing Clang lookup, make output paths and target/options explicit, and return structured diagnostics. Do not silently treat the current helper as a complete plugin system.

## Build cache

Cache only after the basic build is correct. A cache key should include the source file contents, relevant headers, compiler/provider identity and version, target triple, include paths, compiler flags, dependency identities, and relevant environment inputs. Use temporary output files and atomically publish completed artifacts. A failed build must not leave a valid-looking cached artifact.

## Plugin contract: start as an internal interface

Do not begin with arbitrary executable plugins downloaded and run from the network. First define an internal provider interface that can:

- declare its provider ID and version;
- validate its configuration;
- report toolchain requirements and availability;
- produce a build plan from structured inputs;
- execute build steps with explicit arguments;
- return artifacts and native link requirements;
- surface actionable diagnostics.

The first C provider can be built into Fusion. A later release can decide whether third-party plugins are native libraries, executable providers, WASM components, or another sandboxed format. Plugin trust, permissions, signatures, and update policy should be designed before executing third-party plugin code.

## Error handling

Diagnostics should identify the project and phase, preserve the underlying tool's exit status/output, and suggest a fix. Examples:

- `fusion.toml: [foreign.mathlib].sources: file not found: foreign/mathlib.c`
- `C toolchain unavailable: could not locate clang. Install LLVM or configure the toolchain provider.`
- `link failed: unresolved symbol 'mathlib_add'; check exported symbol names and native link inputs.`

Do not swallow the compiler's stderr or replace useful diagnostics with a generic "build failed".

## Implementation sequence

1. **CLI and project discovery:** preserve existing single-file commands; add manifest discovery and `fusion init`.
2. **Manifest model and validation:** parse TOML, validate required fields and safe relative paths, and add unit tests.
3. **Project build orchestration:** make `fusion check/build/run` operate on a manifest-backed project while retaining current compiler entry points.
4. **C provider:** compile a minimal C source, link it into the built application, and prove an end-to-end call from Fusion.
5. **Dependency resolution and lockfile:** begin with local path dependencies; add immutable Git/registry sources only with verification and tests.
6. **Toolchain doctor and diagnostics:** detect missing tools and report versions, targets, and actionable fixes.
7. **Caching and remote builds:** only after build inputs and outputs are modeled precisely.

## Acceptance criteria for the first end-to-end milestone

- A fresh project can be created with `fusion init`.
- The manifest is parsed and invalid configuration produces a useful error.
- A Fusion project can include one local C source file and a header.
- Foundry invokes the configured/discovered C compiler without requiring the user to manually run it.
- The produced executable links the C object and calls a known C function successfully.
- Existing single-file CLI behavior and tests continue to work.
- Missing Clang, missing source files, compiler errors, and linker errors fail cleanly.
- Generated artifacts stay under the project build directory and can be removed by `fusion clean`.
- Documentation labels proposed versus implemented features honestly.

## Non-goals for this milestone

- Supporting every language listed in the long-term vision.
- Automatically translating foreign source into Fusion IR.
- Installing arbitrary system SDKs without user consent.
- A public plugin marketplace or executing untrusted downloaded plugins.
- Claiming bit-for-bit reproducibility before the relevant inputs are modeled.
- Cloud builds, distributed caching, or IDE optimization suggestions.

The guiding rule is: **Foundry owns the project workflow; providers own ecosystem-specific build mechanics; Fusion's language semantics remain independent of both.**
