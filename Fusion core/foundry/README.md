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
