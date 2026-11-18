# Fusion Phase 4 — Technical Baseline & Initial Gap Register

Baseline: `Fusion-phase3-hashmap-typechecker-fix.zip` (Phase 3 full test suite reported passing by project owner; not independently rerun here).

## Scope and verification

Reviewed source structure and implementation/documentation in the baseline archive, including `foreign.rs`, `linker_config.rs`, Foundry sources, `lib.rs`, LLVM backend, type checker, HIR, module system, concurrency extension and project status documentation. ZIP extraction was successful. Cargo is not installed in this environment; no independent test execution is claimed.

## Phase 4 readiness: C interoperability

### Confirmed foundation
- `foreign.rs` defines `ForeignKind`, `ForeignLibrary`, C source/include/library metadata, a `compile_c` helper invoking `clang`, and linker-argument generation (`-L`/`-l`).
- LLVM emission and native executable linking exist in the core compiler.
- Foundry has package graph, process, toolchain, cache, fingerprint, plugin, manifest and test-harness modules.
- LLVM IR uses opaque pointers and supports package/external declarations in limited contexts. Existing package-interface code is not a C FFI implementation.

### Confirmed gap from source inspection
- The standalone `ForeignLibrary` helper is not visibly wired into Foundry's build execution path.
- No Fusion source-language `extern` declaration mechanism was found in the parser/AST/HIR pipeline.
- No complete foreign function ABI signature/type mapping is implemented.
- No end-to-end Fusion-to-C function invocation path is established.
- No FFI ownership/borrow/pin/callback contract exists.
- C header binding generation is absent.
- Windows toolchain selection and end-to-end C interop validation are not demonstrated.

### Phase 4 work packages
1. **4.1 Foreign build pipeline:** integrate C compilation, artifact tracking, link invocation, compiler detection, diagnostics, build caching and tests into Foundry.
2. **4.2 Declaration model:** design and implement foreign declarations in syntax/AST/HIR; define symbol names and calling conventions.
3. **4.3 ABI-safe types:** initially void, fixed-width integer types, f32/f64 and constrained opaque pointers/handles; document unsupported types.
4. **4.4 Native call lowering:** type-check declarations and calls, generate correct LLVM ABI signatures and link/run tests.
5. **4.5 Boundary safety:** ownership, buffer lifetime, cleanup, error mapping and restrictions; no callbacks or complex aggregates in the first release.
6. **4.6 Windows acceptance:** choose and document compiler/linker ABI, validate diagnostics, build/link/run a separate C source and library, CI coverage where practical.

## Architecture decisions required before implementation
- Initial Windows toolchain and ABI (MSVC-compatible versus MinGW-compatible), and supported compiler drivers.
- Foreign declaration syntax and symbol naming rules.
- C ABI supported type matrix and explicit exclusions.
- Linking strategy for object files, static libraries and dynamic libraries.
- Foundry artifact/cache/fingerprint model for C compilation.
- Pointer/opaque-handle and ownership rules, including cleanup and error propagation.
- Whether interpreter FFI is out of Phase 4 scope (recommended: yes; prioritize compiled/native path).

## Cross-cutting baseline gaps
- **Native backend parity:** language features exist in AST/HIR/checker and interpreter but native support remains uneven. Use `PHASE_STATUS.md` and backend tests as the feature matrix; do not assume syntax implies native support.
- **Native runtime/memory:** tracked allocations exist for some aggregates; this is not a complete heap, collector, ownership system or cleanup model.
- **Async:** concurrency/thread-pool infrastructure exists; it is not a suspension/resumption async executor with async I/O.
- **Module/HIR architecture:** significant module and HIR identity work exists, but HIR semantic side tables and compatibility AST paths remain. Continue consolidation rather than reimplementing modules.
- **Standard library:** substantial interpreter-side modules exist; parity and stable cross-backend contracts remain to be audited and tested.
- **Self-hosting:** bootstrap should wait on native runtime, language/backend parity, process/filesystem functionality and a reproducible bootstrap driver.

## Proposed first acceptance test (Phase 4.1)

A Foundry project compiles a separate C source file to an object artifact, links it with a Fusion native executable, and successfully runs a Fusion call to the C function. Build errors include actionable source/toolchain context. This test does not yet require Fusion `extern` syntax; it validates the build pipeline independently.

Illustrative target behavior (syntax not yet specified): C function `int32_t add(int32_t, int32_t)` is compiled and linked, then invoked from Fusion and returns the expected value.

## Status

Audit baseline recorded. No source changes made to Fusion itself. Phase 4.1 is the recommended first implementation task.
