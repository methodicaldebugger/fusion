Foundry - Fusion project/package/toolchain manager

The package manager for Fusion projects. It provides commands to create, build, run, and manage Fusion packages and their dependencies.
It is responsible managing dependency resolution, building Fusion programs, running and testing them, compatibility checks, publishing and more.
It resolves version, dependencies and compatibility issues for plugins.


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
Add further integrations only when the underlying mechanism has a clear benefit.


Foundry should orchestrate, not reinvent.
Use existing, mature infrastructure whenever possible:
Rust's compiler and Cargo.
LLVM for native code generation.
Existing C and C++ compilers and linkers.
Existing managed runtimes.
Existing registries and build systems where appropriate.


Bazel, in particular, already supports multi-language build orchestration, so Foundry should not rely on multi-language builds alone as its novelty.
We should relly on Bazel and Cargo for guidance, knowledge and inspiration.