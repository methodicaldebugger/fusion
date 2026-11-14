# Foundry workspace resolution – continuation

This continuation adds the constraint-solving foundation to the existing registry and lockfile work.

## Implemented in this revision

- Registry requirements now support comma-separated conjunctions, such as `>=1.4.0, <2.0.0`.
- Added `VersionConstraint`, carrying both the requesting package and its requirement.
- Added deterministic `select_version_for_constraints`, which selects the highest published version satisfying every supplied constraint and reports all requesters when the constraints conflict.
- Added `resolve_with_constraints` so a caller with a complete set of workspace requirements can resolve a package once against that set. The existing `resolve` API remains as a compatibility wrapper for one manifest requirement.
- Kept the single exact-version/offline path intact: an exact lockfile pin does not need the registry versions index; cached metadata, archive bytes, and checksum verification are still required.
- Added regression tests for compound ranges, common-version selection, and conflict diagnostics.

## Important remaining integration work

The current dependency-tree traversal still invokes `resolve_dependencies_one` package by package. It does **not yet collect all transitive registry constraints before materializing packages**, nor does it rewrite all package manifests to consume one workspace-wide selected version. `resolve_with_constraints` is the new resolver entry point for that next integration step, not proof that the full workspace-wide policy is already active.

The existing per-package lockfiles and Git commit pinning behavior were not redesigned in this revision. Incremental compilation and additional package-graph invalidation tests remain subsequent work.

## Validation status

The Rust source and regression tests were updated, and the workspace archive was checked for ZIP integrity. `cargo test` could not be run in this environment because `cargo`/`rustc` are not installed; the new Rust tests are therefore added but unexecuted here.


## Incremental package change planning (continued)

- Added a persisted per-profile/target package fingerprint state file at `target/<profile>/.foundry-package-fingerprints.toml` (target-specific output directories naturally isolate target state).
- Foundry compares current dependency-aware package keys with the last successful build and classifies packages as changed, unchanged, or removed.
- The state is written only after a successful local compilation or a successful remote-cache restore; a failed compile cannot mark new package keys as built.
- State writes use a temporary file followed by rename. On Windows, the existing state file is removed first because the standard filesystem rename does not replace an existing destination there.
- Added tests for all three change classes and state-file round-tripping/replacement.

At this stage package-level artifact production had not yet been connected. The later “Per-package LLVM artifact cache” section below supersedes this historical limitation: Foundry now emits and verifies independent LLVM IR translation units, although final package-level linking is still pending.

## Validation update

Source-level inspection and archive integrity checks were performed. Rust tests remain unexecuted because this environment has no `cargo` or `rustc` executable.

## Incremental cache-state recovery (this revision)

- Fixed the local fresh-build fast path: when the executable and its build fingerprint still match, Foundry now also repairs the package fingerprint state file before returning the cached artifact.
- This covers interrupted/manual cleanup scenarios where the executable and `.foundry-fingerprint` survive but `.foundry-package-fingerprints.toml` is missing or stale.
- The repair helper refuses to update package state unless the artifact exists and the stored build stamp exactly matches the expected build fingerprint.
- Added a regression test proving that a fresh artifact reconstructs missing package state and that a mismatched stamp cannot overwrite existing state.

This is a cache-consistency fix, not a claim of per-package artifact compilation. Workspace-wide constraint collection and per-package artifact production remain open Phase 3 tasks.

## Validation update

The changed Rust source was inspected and the regression test was added. `cargo test` and `rustfmt` could not be run because this environment does not have the Rust toolchain installed.


## Workspace-wide lock consistency guard (this revision)

- After the complete runtime dependency tree has been resolved and materialized, Foundry now builds the package graph and inspects registry requirements and lock entries across every reachable package.
- It rejects a workspace that has different locked versions for the same registry dependency alias, even when each package's individual lockfile is locally valid.
- It also checks that the shared locked version satisfies every package's declared requirement and reports the requesting packages and ranges in diagnostics.
- Added regression tests for a common compatible pin, mixed pins, and a shared pin that cannot satisfy all requirements.

This is an enforcement/diagnostic step, not yet a workspace solver: it detects divergent per-package resolutions after materialization; it does not proactively choose one shared version and propagate it into all package lockfiles. `foundry update` still resolves each package manifest independently. The next step is to make the workspace constraint set an input to resolution itself, then write consistent lock entries.

## Validation update

The workspace graph/lock validation path and focused regression tests were added. ZIP integrity and source-level structural checks passed. Rust tests could not be executed because `cargo`, `rustc`, and `rustfmt` are unavailable in this environment.

## Package graph regression coverage (this revision)

- Added a diamond-dependency test proving a shared path package is represented by one graph node while both parent-to-shared edges are retained.
- The test also asserts dependency-first build ordering for both branches and the root package.
- Added a path-dependency cycle test that checks the graph builder reports the participating canonical package paths.
- These tests exercise the graph builder used by workspace lock validation and provide a stable contract for the next workspace-wide resolution pass.

The registry lock format currently records the source kind as `registry`, not the registry URL or canonical registry package identity. Therefore this revision deliberately does not attempt to group renamed aliases across registries using information the lockfile does not contain. A future lockfile format change should persist registry identity and canonical package name before adding that behavior.

## Validation update

The new tests and source changes were inspected, and the archive was checked for ZIP integrity. Rust tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable in this environment.

## Incremental dependency invalidation (same revision)

- `PackageGraph::affected_packages` now computes the transitive reverse-dependency closure for changed package roots.
- `PackageGraph::rebuild_order_for_changes` filters the existing dependency-first order to only packages affected by those changes.
- The diamond graph regression now verifies that changing a shared leaf rebuilds the leaf, both intermediate packages, and the root in dependency-first order.
- Unknown changed roots are ignored. Removed nodes and changed/removed edges still require old-vs-new graph comparison by the caller; this API intentionally does not claim to solve graph-delta invalidation yet.

## Package graph delta and removal invalidation

The package graph now exposes `PackageGraph::delta_from(previous)`, returning a
`PackageGraphDelta` with changed package roots, removed package roots, and a
current-graph dependency-first rebuild order. It detects added/removed nodes,
node metadata changes, and dependency-edge additions/removals/rewires. An edge
change invalidates its consuming package, then the existing reverse-dependency
closure propagates invalidation to all transitive consumers. Removed package
roots are reported separately so the build layer can garbage-collect artifacts
that are no longer present in the current graph.

Regression coverage now includes removal of a dependency from a workspace and
addition of a package. The removal test verifies that the removed node is
reported while its still-present consumer is scheduled for rebuild; this closes
the graph-delta gap where the changed node itself no longer exists in the new
graph. The delta API is currently a graph-level primitive; persistence of graph
snapshots and per-package artifact garbage collection remain follow-up work.

## Persisted package-graph snapshots (this revision)

- Added a portable TOML snapshot representation for package graphs. Nodes and edges are stored as ordered arrays rather than path-keyed TOML tables, preserving package identity, aliases, endpoints, and dependency-first build order.
- Foundry now stores `.foundry-package-graph.toml` beside the per-package fingerprint state in each profile/target output directory.
- Each build compares the newly resolved graph with the last successful graph snapshot and reports changed/removed graph nodes plus the current-graph rebuild closure. The existing dependency-aware package fingerprints remain the authoritative cache keys; graph deltas add topology-aware invalidation diagnostics and a durable record for future package artifact cleanup.
- Graph snapshots are committed after successful local compilation, successful remote-cache restoration, or validation of a fresh local artifact. The fresh-artifact recovery path repairs both package fingerprint state and graph snapshot state.
- Added round-trip tests for graph snapshots, including node/edge/order preservation, plus state-file replacement and fresh-build recovery coverage.

The compiler still emits one root application artifact. Graph snapshots do not yet provide per-package artifact storage, independent package compilation, or garbage collection. The Rust toolchain is unavailable in this environment, so the added tests have not been executed; source inspection and archive integrity checks are the available validation.

## Package artifact cache lifecycle foundation (this revision)

- Added a content-addressed package artifact-record store under each profile/target output directory at `.foundry-package-artifacts/<package-root-hash>/artifact.toml`.
- Each record binds the canonical package root, package name/version, record format version, and the existing dependency-aware package fingerprint. Cache-hit checks reject stale or mismatched fingerprints.
- Records are committed only through the successful package-state persistence path, which is reached after successful local compilation, successful remote-cache restore, or validation of a fresh local executable. Failed compilation therefore cannot publish new package records.
- Added conservative garbage collection for removed packages. It only examines hash-named directories inside Foundry's dedicated store and deletes entries only when they contain a valid Foundry v1 record whose package root is no longer in the current graph. Unrecognized files/directories are left untouched.
- Added tests for content-addressed records, fingerprint mismatch rejection, and removal cleanup.

**Historical boundary:** at this point the store contained metadata records only. The later “Per-package LLVM artifact cache” section below supersedes this state and records the subsequent addition of compiler-produced IR artifacts.

## Validation update

The source changes and regression tests were inspected, and the archive passed `unzip -t`. Rust tests and formatting checks remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable in this environment.


## Per-package compiler artifact foundation

- Foundry now invokes the Fusion compiler's `emit-llvm` mode for each package in dependency-first graph order, passing that package's direct dependency roots.
- The resulting LLVM IR translation units are stored independently under `.foundry-package-artifacts/<package-root-hash>/<package-fingerprint>.ll`.
- Cache records now include artifact kind, relative artifact path, and SHA-256 digest. A cache hit requires matching package identity/fingerprint, the expected IR format, and verified artifact bytes; missing or corrupted outputs are re-emitted.
- Artifact generation is staged through a temporary file and renamed into place. Records are committed with the successful root build, and stale package directories are pruned only from Foundry's own hash-named store.
- Added regression coverage for artifact integrity: a fingerprint match with corrupted IR bytes is not accepted as a cache hit.

**Current boundary:** these are independently compiled LLVM IR units, not yet independently linked object files or final package libraries. The ordinary root `fusion build` still compiles and links the complete application graph. Foundry now has real per-package compiler outputs to reuse and a verified cache contract; the next step is an explicit compiler/linker interface that can consume cached package IR/object outputs during the final application build.


## Package-scoped public symbol interface (this revision)

- Added the compiler API `emit_package_interface_with_package_roots` and the `fusion package-interface` CLI command. The command compiles/resolves the project with the normal module and visibility checks, then emits a deterministic `FUSION-PACKAGE-INTERFACE\t1` manifest for modules physically contained under the selected package source root.
- Interface manifests contain module identities, public exports, import aliases/selections, and re-export visibility. Private declarations are not exposed as exports. Output ordering is deterministic and fields are escaped for the line-oriented format.
- Foundry now emits and caches a `.fpi` interface beside each package LLVM cache artifact. Cache records were upgraded to format v3 and include the interface relative path and SHA-256 digest; cache hits require both the IR artifact and interface manifest to pass integrity checks. Missing/corrupted interfaces force regeneration.
- Added compiler coverage proving that only public declarations from the selected source root appear as exports, plus Foundry cache tests for interface integrity and path traversal rejection.

**Backend boundary:** this increment establishes the package-scoped symbol/interface contract and binds it to the package cache, but it does not yet emit independently linkable package-only LLVM modules. The existing LLVM backend consumes a flattened project AST and requires an application `main`; it also emits shared runtime support. Splitting this safely requires explicit external declaration generation and runtime ownership in the backend. Cached `.ll` files therefore remain whole-project translation units; the `.fpi` files are package-scoped metadata, not linker symbol tables with ABI signatures. The next backend increment must introduce typed exported/imported symbol signatures and module-scoped IR emission before independent package linking can be enabled.

## Validation update

The CLI/API integration, cache record version upgrade, and regression tests were inspected. Rust tests and formatting checks remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable in this environment.


## Package ABI signatures and external declarations

- Upgraded generated package interfaces to `FUSION-PACKAGE-INTERFACE	2`.
- Public, locally defined functions with explicit scalar parameter and return types now carry deterministic `abi-function` records. The supported initial ABI maps `num`/`i64` to LLVM `i64`, `float`/`f64` to `double`, `bool` to `i1`, and `string`/`str` to `ptr`; `void` is supported only as a return type.
- Generic, async, aggregate, missing-type, and otherwise unsupported signatures are recorded as `abi-unsupported`; the compiler does not invent a layout or calling convention for them.
- Added `emit_package_external_declarations` and the `fusion package-declarations <input.fpi> [-o declarations.ll]` command. They validate v2 ABI records, reject malformed symbols and conflicting duplicate declarations, and emit deterministic LLVM `declare` statements.
- Foundry now rejects cached v1 package interfaces and regenerates them, so stale metadata-only interfaces cannot be mistaken for ABI-capable interfaces.
- This is an ABI contract foundation, not yet package-only LLVM emission: the main LLVM backend still emits the existing flattened project translation unit, and Foundry does not yet substitute independently emitted external declarations into package modules.
- Added regression tests for scalar ABI records, generated declarations, unsupported signatures, malformed symbols, conflicting signatures, and interface-version rejection. Rust tests were not executable in this environment because `cargo`/`rustc` are unavailable.

## Package-scoped LLVM module emission (this revision)

- Added `LLVMBackend::emit_library_program`, which shares the normal backend pipeline but does not require an application `main`. Ordinary executable emission still requires `main`.
- Added `emit_llvm_package_with_package_roots` and the `fusion emit-package-llvm <entry> --package-source-root <src>` command. The compiler performs normal project loading, name resolution, HIR/type validation, and LLVM lowering, then selects function definitions whose injective module-mangled symbols belong to the requested package source root.
- Package units include the required LLVM type declarations and platform/runtime external declarations. Calls to functions in other Fusion modules receive declarations derived from the corresponding compiler-emitted function signatures; the selected package's own function bodies remain definitions.
- The first package-unit boundary is intentionally conservative: package functions that reference shared `@fusion_*` runtime helpers or package/global data are rejected with a specific diagnostic. This avoids silently emitting incomplete modules or duplicating process-global runtime state across packages. Scalar, self-contained functions are the supported initial slice.
- Added a regression test with two imported modules to verify that the requested package's functions are emitted, unrelated module definitions are excluded, and the application `main` is omitted from a library unit.

**Integration boundary:** Foundry's final executable still links the existing root application translation unit. The new package-scoped command is an independently callable compiler primitive; it is not yet substituted into Foundry's cache-to-link path. The next increment must add shared runtime ownership/object linking and consume these package units together with ABI-checked imports. Package functions using runtime helpers or global data remain intentionally unsupported by this first slice.

Rust tests and formatting checks remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable in this environment. The regression test is included but is not claimed to pass.


## Package modules linked with a separate entrypoint and shared runtime (this revision)

- Foundry now compiles each graph node with `emit-package-llvm` and its own canonical source root. The cached `.ll` artifact is now a package-scoped module rather than a whole-project translation unit; the cache record format was upgraded to v5 and the artifact kind to `llvm-package-ir-v3` so older whole-project artifacts cannot be mistaken for package modules.
- Added `emit_llvm_entrypoint_with_package_roots` and `fusion emit-entrypoint-llvm`. This emits the application's `main` plus shared runtime definitions, replacing package-owned function bodies with external declarations.
- Shared runtime helpers (`fusion_heap_alloc`, tuple helpers, cleanup helpers) and their runtime state globals now have external linkage in the entrypoint module. Package modules emit external declarations for runtime helpers they call, so runtime state is owned once by the entrypoint rather than duplicated in every package.
- `fusion link-llvm` now accepts multiple LLVM IR inputs. Foundry links the entrypoint/runtime module together with every package module in dependency-first graph order. This is the first end-to-end package-module link path; package artifacts are now actual linker inputs rather than only cached side products.
- Added an entrypoint artifact cache keyed by the root package fingerprint, with a SHA-256 sidecar. A changed dependency invalidates the entrypoint key as well as downstream package keys; unchanged package modules remain independently reusable.
- Added regression coverage for separating package function definitions from `main`, retaining shared runtime definitions in the entrypoint module, emitting external package declarations, and validating every `link-llvm` input before invoking Clang.

**Current limits:** package-scoped LLVM emission still rejects package functions that directly reference package/global data because there is not yet a package data ABI. The initial typed function ABI covers only supported scalar signatures; generic, async, aggregate, and inferred signatures still need richer ABI metadata. Interface-to-definition verification and full end-to-end execution tests remain the next hardening step. The Rust test suite could not be run here because `cargo` and `rustc` are unavailable; tests were added but are not claimed to pass.


## Package link ABI enforcement (November 2026)

Foundry now validates package-scoped LLVM units against their v2 `.fpi` ABI manifests before attempting either a fresh native link or a fresh/remote executable cache return. Validation checks that every supported public ABI export has a matching LLVM definition in its owning package unit, compares the interface signature to the emitted definition, rejects duplicate LLVM definitions, and checks every cross-unit Fusion (`_F...`) declaration against exactly one package provider with an identical signature. Runtime symbols remain outside this package ABI check because the application entrypoint/runtime unit owns them. Unsupported/private imported Fusion symbols fail with a package-level diagnostic instead of being deferred to Clang. Regression tests cover matching contracts, consumer signature drift, missing exported definitions, unresolved declarations, and duplicate definitions.

The current interface still describes typed scalar functions only. Package-owned global/static data, aggregate layouts, generic functions, and method ABI records remain unsupported; package code that relies on those forms is not silently treated as having a complete ABI. Rust tests were added but must be run in an environment with the Rust toolchain.

The v2 interface now also records typed private scalar functions as `abi-internal-function` (and explicitly unsupported private functions as `abi-internal-unsupported`). This is required because the application entrypoint and root package code are emitted as separate LLVM units: the entrypoint may legitimately call a private helper belonging to the root package. Foundry permits that one ownership case while rejecting dependency-private symbols from other package units. Public exports remain the only symbols exposed for normal cross-package use.

The package artifact record/cache kind is now v5 / `llvm-package-ir-v3`, forcing older v2-only interface caches to be regenerated so private scalar ABI records cannot be silently absent after an upgrade.


### Link-time ABI contract checks

Before returning a fresh/remote executable cache result or invoking Clang, Foundry now verifies each package `.fpi` against the package's actual LLVM definitions. It checks exported scalar function signatures, rejects duplicate definitions, verifies that every used cross-unit Fusion function declaration has exactly one ABI provider, and compares consumer declarations with provider signatures. Unused declarations preserved by the current extraction stage are ignored until they are referenced by an LLVM instruction. Private scalar functions receive internal ABI records so root `main` can call root-private helpers across the entrypoint/package-unit boundary; dependency-private symbols remain inaccessible to other package units. Cache records were bumped to v5 / `llvm-package-ir-v3` to force regeneration of older interface artifacts.

Remaining limitations: global/static data ownership, aggregate type layouts, method/generic/async ABI coverage, and full compiler-backed end-to-end tests.

## Package-owned data ABI metadata (November 2026)

Package interfaces are now v3. Public scalar literal constants have stable package/module-qualified mangled data symbols and explicit scalar type/value encodings. Public structs have stable type symbols, ordered field metadata, and a SHA-256 digest over their canonical field layout. Foundry validates these records before accepting fresh or cached package artifacts, including duplicate identity checks, field-count and field-name checks, symbol validation, digest verification, and scalar encoding/type consistency.

This is the data ABI contract foundation only: constants are not yet emitted as package-owned LLVM globals, imported constant references do not yet bind to those symbols, and aggregate values still lack a native cross-package layout implementation. Package-scoped LLVM emission therefore continues to reject package/global data references. Next: lower supported scalar constants to package-owned LLVM globals, emit matching `externally_initialized`/external declarations as appropriate, and connect imported references to the ABI symbol; then add aggregate layout lowering.


## Package-owned scalar data emission and linking (this revision)

- The LLVM backend now lowers module-scoped scalar literal constants to deterministic package-owned LLVM globals. The symbol is derived from the constant's HIR declaration identity and matches the package interface's `mangle_function(module, "$data$<name>")` convention. Integer, floating-point, boolean, and string literals are supported; string globals point at private literal byte arrays.
- Constant identifier lowering now loads from the corresponding global symbol when the identifier resolves to a module constant. Local variables and local constants continue to use stack slots, so lexical shadowing remains distinct from package globals. Non-literal module constants are explicitly rejected when referenced by the LLVM backend rather than silently lowered incorrectly.
- Package-scoped LLVM extraction keeps the selected package's data definitions, includes private literal byte storage, and emits `external constant` declarations for referenced data owned by other modules. The entrypoint emits external declarations for package data it references instead of defining duplicate global copies.
- Package interfaces are now v4 and include typed `abi-data` records for public constants and `abi-internal-data` records for package-private constants. Foundry validates each data provider against the actual LLVM global definition, checks scalar type agreement, rejects duplicate providers, resolves external data declarations, and enforces visibility (including the special entrypoint-to-root-private case).
- Package artifact kind was bumped to `llvm-package-ir-v4` so cached modules generated before data-global lowering cannot be reused with the new linker contract.
- Added compiler regression coverage for package-owned integer/boolean/string globals, package function loads, entrypoint external declarations, public/private data interface records, and Foundry tests for valid data providers, type mismatches, and dependency-private data access.

**Current boundary:** this is real scalar constant data emission and cross-module linking, not general static storage. Non-literal initializers, mutable globals, aggregate values, and native struct layout imports remain unsupported. Rust compilation/tests could not be run because `cargo` and `rustc` are unavailable in this environment; a separate Clang IR/object-link smoke test confirmed the emitted pattern for scalar globals and pointer-valued string constants is accepted by Clang.


## Phase 3 increment — stable aggregate type ABI and layout enforcement

- LLVM identified struct/enum type names are now derived from stable module + nominal declaration identities instead of project traversal counters. This prevents unrelated package additions from renumbering type names and invalidating cross-package layout references.
- Package interface format v5 records the exact LLVM storage layout for supported struct fields and uses the same stable identified-type name emitted by the backend. Scalar fields and pointer-represented nominal fields are supported; arrays and other unsupported aggregate storage remain explicit and are rejected for cross-package linking.
- Foundry validates each exported struct interface against the concrete identified LLVM type emitted in its package module. It also compares repeated identified-type declarations across package modules and the entrypoint, rejecting conflicting layouts before Clang is invoked.
- Package artifact record format is v6 and package IR kind is `llvm-package-ir-v5`, so artifacts produced with traversal-order type names cannot be reused.
- Added a regression test for matching and mismatching emitted LLVM struct layouts. Rust tests were not run in this environment because `cargo`/`rustc` are unavailable.

## Aggregate ABI link-gate regression hardening (November 2026)

- Added Foundry regression tests that exercise the same `validate_package_link_inputs` gate called by `build_inner` immediately before `link-llvm`.
- The struct negative case supplies a valid v6 public struct interface but an emitted identified LLVM type with a different field representation. The link gate must reject it with a struct ABI mismatch.
- The enum negative case supplies a valid v6 enum header and payload interface but emits the payload identified type with the wrong storage field. The link gate must reject it with an enum payload ABI mismatch.
- These complement the lower-level layout parser tests: they demonstrate that the complete pre-link validation entrypoint catches aggregate drift, not merely that an individual helper can detect it.
- The existing build orchestration calls `validate_package_link_inputs` before constructing/running the `link-llvm` process, so a validation error exits before native linking and before successful build state is committed.

**Validation limitation:** the new tests are source-level additions. ZIP integrity and manifest parsing can be checked here, but Rust compilation/test execution remains unavailable because this environment has no `cargo`, `rustc`, or `rustfmt` executable.

**Next Phase 3 target:** run a full Foundry build fixture with a real Rust-built Fusion compiler, then add cache-corruption and cache-hit coverage for the aggregate ABI interface/artifact pair. The native Clang test in the compiler crate and these Foundry pre-link tests are still distinct layers until that full build fixture is executed.


## Phase 3 increment — package artifact cache corruption recovery

- Added a Foundry compiler-harness regression that drives `compile_package_artifacts` with a deterministic fake Fusion compiler. It produces package LLVM and v6 interface artifacts through the same command interface used by the build orchestrator.
- The test establishes a cache record, verifies a valid artifact/interface pair is reused without invoking the compiler again, then independently corrupts the cached LLVM bytes and the cached interface bytes. Each corruption must invalidate the cache entry and regenerate the package artifacts before they can reach the pre-link validation gate.
- The test checks compiler invocation counts across the cold build, warm cache hit, LLVM corruption recovery, and interface corruption recovery. This covers recovery behavior rather than only checking that a checksum mismatch is detectable.
- This remains a focused orchestration-layer test, not a complete `build_inner` invocation: dependency resolution, entrypoint generation, ABI pre-link validation, native linking, and executable runtime are still separate integration layers.

**Validation limitation:** the Rust regression has been added but could not be executed here because `cargo`, `rustc`, and `rustfmt` are unavailable. The archive and Cargo manifest checks are performed separately and do not establish that the Rust test compiles or passes.


## Phase 3 increment — full Foundry build orchestration regression coverage

- Added Unix Foundry integration fixtures that invoke the production `build_inner` orchestration with a deterministic fake Fusion compiler and a real two-node path-dependency graph (`model` followed by `app`). The fixture exercises package graph resolution, dependency-first package compilation, package-interface generation and validation, entrypoint artifact generation, package-link input validation, the `link-llvm` command boundary, and successful package/graph/fingerprint state publication.
- The successful-build test invokes `build_inner` twice. It verifies that the first invocation compiles both package modules and the entrypoint, invokes the linker, and commits all package records and state. The second invocation reuses the package cache and matching executable fingerprint without recompiling or relinking.
- Added a malformed-interface case that verifies the build stops before invoking `link-llvm` and does not publish successful package/graph/executable state.
- Added a native-link failure/retry case. The first link attempt fails and must leave package artifact records, package fingerprints, graph state, and the executable success stamp uncommitted. After the fake linker is allowed to succeed, a retry rebuilds the uncommitted package modules, links successfully, and publishes the expected state.

**Scope and validation boundary:** these are full `build_inner` orchestration tests, but they use a fake compiler/link command and therefore do not prove that a Rust-built Fusion compiler emits executable LLVM for this fixture. The path-dependency graph is real; registry/network dependency resolution is not exercised (`resolve=false`). Rust tests remain unexecuted in this environment because `cargo`, `rustc`, and `rustfmt` are unavailable.


## Phase 3 continuation — incremental graph invalidation and last-known-good executable safety

- Native linking now targets a unique sibling staging file instead of the currently published executable. A failed linker, a missing staged output, or a failed package-state commit cleans up the staging file and leaves the previous executable untouched. On successful linking, package records and graph/fingerprint state are committed before the staged executable replaces the published output; the executable fingerprint stamp is written atomically last.
- Remote-cache restoration also downloads to a staging path, so a failed or incomplete restore cannot truncate the last known-good executable. Successful restore follows the same metadata, executable replacement, and final-stamp ordering.
- Added a `build_inner` incremental regression using a real two-package path-dependency graph and the fake compiler harness. Editing the dependency invalidates its own key and the root key through dependency-aware fingerprint propagation. A forced link failure must preserve the prior executable bytes, build stamp, package fingerprint state, graph snapshot, and committed dependency record. A retry rebuilds the uncommitted package artifacts and succeeds.
- The same test removes the dependency from the root manifest, reloads the project, rebuilds, and verifies that only the remaining root package is recompiled and the removed package's stale cache record is collected after successful linking.
- Validation performed for this archive: ZIP integrity, Cargo manifest TOML parsing, shell syntax for the fake compiler fixture, and source-level assertions. Rust compilation and tests were not run because `cargo`, `rustc`, and `rustfmt` are unavailable in this environment. These tests are added to the real Foundry `build_inner` path but still use a deterministic fake compiler/link command; they do not substitute for a real Rust-built Fusion compiler end-to-end run.


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


### Phase 3 continuation — linker search-path and environment provenance (format v4)

- Extended the Clang identity used by Foundry fingerprints with `-print-search-dirs` output and its digest. This records the compiler driver's discovered program/library search configuration in addition to its binary, version, target triple, and resource directory.
- Captured a bounded allowlist of toolchain-affecting environment variables (SDK selection, Clang config overrides, compiler/library/header search paths, dynamic-library paths, and `SOURCE_DATE_EPOCH`). Set and unset states are both explicit and contribute to the root executable fingerprint.
- Build provenance format advanced from v3 to v4 and now stores the linker search-directory output/hash and the allowlisted environment values. Local freshness validation rejects older provenance formats and compares the complete expected document.
- Added regression assertions for search-directory identity, environment capture, and provenance differences caused by changed search paths or SDK environment values.
- The actual Fusion compiler/native multi-package execution milestone remains unverified: this environment still has no `cargo` or `rustc`, so Rust tests cannot be run here.

## Phase 3 CI gate — real compiler/native integration

Added a GitHub Actions workflow that checks formatting, runs all targets for both the Fusion compiler and Foundry crates, then invokes the real-toolchain three-package native smoke harness. The workflow preserves toolchain details, smoke logs, and the generated fixture workspace/provenance as a 30-day artifact, including on failure. See `docs/phase3-ci.md` for the required branch-protection check name and evidence layout. The workflow must run on GitHub before its native integration status can be considered verified; this editing environment has no Rust toolchain.


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

### Phase 3 continuation — registry extracted-cache integrity and safe recovery (2026-11-13)

- Registry package names and resolved versions are validated as safe cache path components before they are used in cache paths.
- The resolver continues to verify the exact archive bytes against the registry/lockfile SHA-256. It now independently fingerprints extracted package trees using sorted relative paths, entry kinds, permission identity, and regular-file contents, and records the result in a versioned sibling tree-manifest file bound to the archive checksum.
- Every resolution verifies the extracted directory against its tree manifest. Missing, malformed, archive-mismatched, or content-mismatched manifests trigger re-extraction from the already checksum-verified archive. This repairs modified source files and injected extra files even during offline resolution.
- Archive preflight rejects absolute paths, parent traversal, Windows path separators/drive prefixes, symbolic and hard links, and special filesystem entries before extraction. The current registry archive contract is therefore intentionally limited to ordinary directories and regular files.
- Archive downloads are also staged to unique temporary sibling files and published only after a successful transfer. Re-extraction occurs in a unique staging directory. The old extracted directory and tree manifest are moved to backups and restored if publishing either new object fails. A malformed archive or failed extraction leaves the previous extracted directory untouched.
- Added registry tests for offline initial extraction, edited-file and injected-file repair, corrupted tree-manifest recovery, malformed archive preservation, unsafe package-name rejection, and symlink archive rejection on Unix.
- Added `foundry/docs/registry-cache-integrity.md` describing the tree manifest, offline recovery behavior, archive restrictions, and trust boundary. The local tree manifest detects ordinary cache corruption but is not a signature against an attacker able to rewrite both the cache tree and its manifest.
- Validation in this environment: source inspection, TOML/YAML-independent static checks, and archive integrity. Rust compilation/tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. The real native compiler integration remains pending the Rust-equipped CI gate.

## Registry cache concurrency and lifecycle

- Added cross-process per-package registry cache locking around index, metadata, archive, and extracted-tree resolution. Stale locks are reclaimed only when their recorded PID is confirmed absent; ambiguous lock records fail closed.
- Added targeted cleanup of abandoned extraction, backup, and temporary-manifest siblings after a verified package tree is present.
- Added `foundry cache prune`, scoped to the current workspace's validated `foundry.lock`. It retains locked registry versions, serializes deletion with package resolution, and rechecks the lockfile after acquiring the package lock.
- Registry caches remain workspace-local, so pruning one project cannot remove another workspace's versions.
- Added tests for stale lock recovery, current-process liveness, and package-scoped cleanup. Rust compilation and tests still require a Rust toolchain unavailable in this environment.


### Phase 3 continuation — registry cache observability (2026-11-13)

- Added `foundry cache inspect` as a read-only diagnostic command for workspace-local registry caches, with per-package health states and actionable detail for malformed metadata, archive checksum problems, missing cache layers, and extracted-tree integrity failures.
- Added `foundry cache inspect --json` for machine-readable reporting without changing cache state.
- Inspection validates metadata identity/path safety, archive SHA-256 against registry metadata, and extracted-tree SHA-256 against the local tree sidecar. The sidecar remains a corruption-detection mechanism, not a signature.
- Added tests for healthy cache reporting, archive-versus-tree corruption classification, malformed metadata handling, interrupted stale-lock reclamation, and pruning edge cases that preserve malformed/unindexed entries.
- Documented the difference between inspection, verified recovery via `foundry fetch`, and removal via `foundry cache prune`.


### Phase 3 continuation — registry protocol hardening (2026-11-13)

- Versioned registry package metadata as format 1. `format_version` is mandatory and integer-valued; `name` and `version` are mandatory exact identity fields, and `archive` plus SHA-256 `checksum` are validated before cache publication. Unsupported schema versions fail with an actionable diagnostic.
- Restricted network registry and archive access to HTTPS, including HTTPS-only redirects. Rejected HTTP/FTP/unknown schemes, URL credentials, URL fragments, relative archive paths, whitespace/control characters, and non-absolute local registry paths. Absolute local directories, absolute archive paths, and `file://` remain supported for local registry development and deterministic tests.
- Added bounded curl connection/overall timeouts, a small retry budget, and clearer transport diagnostics.
- Registry version-index and metadata cache writes now use unique staging files and atomic publication. Invalid metadata is validated before replacing an existing cached metadata record, preserving last-known-good state on malformed responses.
- Added tests for schema version/identity enforcement, unsafe URL rejection, local registry range resolution through `versions.txt`, and preservation of cached metadata after a malformed online response.
- Validation in this environment: source-level checks, TOML parsing of project manifests, documentation checks, and ZIP integrity. Rust compilation and tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. HTTPS behavior and real remote registry interoperability remain pending execution in the Rust-equipped CI environment.
