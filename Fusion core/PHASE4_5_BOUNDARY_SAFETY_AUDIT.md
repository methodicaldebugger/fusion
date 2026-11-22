# Fusion Phase 4.5 — Foreign Boundary Safety Audit

Audit input: latest `Fusion core` archive supplied after the successful Windows `fusion_helper(21) -> 42` acceptance run.

## Scope and verification

This is a source inspection of the uploaded archive. The Windows acceptance run is reported by the project owner; it was not rerun from this environment. No claim is made that Cargo tests or a Windows build were executed during this audit.

The inspected implementation includes `foreign.rs`, `type_checker.rs`, `module_system.rs`, `hir.rs`, `llvm_backend.rs`, and the Phase 4.1–4.4 design notes.

## Confirmed current guarantees

- Foreign declarations are restricted to `extern "C"`.
- Foreign functions require explicit parameter types.
- The accepted ABI subset is fixed-width signed/unsigned integers (8/16/32/64), `f32`, `f64`, and `*opaque`; `void` is allowed only as a return type.
- Generic and async foreign declarations are rejected.
- `ForeignFunctionDecl::validate()` checks nonempty source/link names and the supported parameter/return type subset.
- The LLVM backend emits `declare ccc` declarations and lowers calls using the recorded ABI spelling reconstructed from the AST.
- Narrow integer arguments are truncated from Fusion's `i64` representation; narrow signed/unsigned returns are sign-/zero-extended. `f32` values are converted to/from Fusion's `double` representation.
- The initial boundary does not support aggregates by value, variadics, callbacks, strings/slices, or non-C calling conventions.

## Safety gaps found in the current source

### 1. Validated ABI metadata is discarded by the type checker

`TypeChecker` constructs and validates a `ForeignFunctionDecl` while registering a declaration, but stores only the function in the ordinary `FunctionType` map and its name in `foreign_functions: HashSet<String>`. The validated `ForeignFunctionDecl` itself is not retained. The LLVM backend separately reconstructs the ABI signature from AST type spellings.

**Risk:** there is no single retained, authoritative foreign signature object shared by declaration validation, call checking, and lowering. Future boundary rules could drift between these paths.

### 2. Fixed-width numeric types are intentionally treated as broadly compatible

`types_compatible()` considers any fixed-width integer spelling compatible with any other fixed-width integer spelling, and likewise treats `f32` and `f64` as mutually compatible. This is broader than exact C parameter typing. For example, an `i64`-annotated value can be accepted for an `i8` parameter.

**Risk:** the backend then applies an unchecked LLVM `trunc` or `fptrunc`. Values outside the destination range can silently change, and signedness/width mismatches are not diagnosed at the foreign call site.

### 3. Narrowing conversions are unchecked

`emit_foreign_call()` directly emits `trunc` for integer parameters narrower than 64 bits and `fptrunc` for `f32`. There is no runtime range check or explicit checked-conversion operation at this boundary. The Phase 4.4 note documents truncation semantics, but Phase 4.5 needs to decide whether such conversions are allowed implicitly, require explicit conversion, or are checked.

### 4. Opaque pointers have no ownership or lifetime contract

`*opaque` is represented as `Type::Generic("*opaque")` for type checking and as an LLVM pointer. The current code passes it directly. There is no borrow/ownership annotation, lifetime tracking, nullability contract, release function association, or automatic cleanup.

**Risk:** Fusion cannot establish whether a handle remains valid, whether it may be retained by C, or who must release it. This must remain explicitly caller-managed until a contract is designed.

### 5. `void` call results are represented internally by a placeholder

The LLVM backend emits a `void` call and returns the string `"0"` from `emit_foreign_call()` as its internal expression result. The type checker knows the declared return type is `Void`, but the backend's placeholder deserves explicit regression coverage to ensure a void foreign call cannot be consumed as a value.

### 6. Foreign failures and callbacks are outside the boundary contract

There is no cross-boundary error translation, exception containment, callback ABI, or panic/unwind policy in the initial C subset. The first release should reject unsupported forms and document that C functions must not unwind across the Fusion boundary.

## Phase 4.5 implementation sequence

### 4.5.1 — Authoritative signatures and call validation

- Retain validated `ForeignFunctionDecl` values in the type-checking environment rather than retaining only names.
- Make foreign call checking use that exact signature.
- Define and test argument compatibility rules for fixed-width integer and floating-point parameters. In particular, do not silently treat all integer widths and both float widths as identical at the C boundary.
- Add static range diagnostics for integer literals where the literal value is known. Decide separately how dynamic numeric narrowing is expressed; do not claim it is runtime-checked until such checks exist.
- Add negative tests for arity, incompatible types, out-of-range literals, and attempts to use `void` results as values.

### 4.5.2 — Opaque handle contract

- Specify non-owning/borrowed versus owned handles before adding automatic cleanup.
- Initially keep raw `*opaque` caller-managed and non-dereferenceable by Fusion.
- Define null handling and whether handles may be stored or returned.

### 4.5.3 — Error and unwind policy

- Define the supported C error pattern (for example, status/result codes) without pretending C exceptions or panics can safely cross the ABI.
- Reject callbacks and unwind-capable boundary behavior until explicitly implemented.

### 4.5.4 — Boundary conformance tests and documentation

- Add compile-time negative tests and native integration tests for the supported subset.
- Document guarantees separately from caller responsibilities and unsupported behavior.

## Recommended Phase 4.5.1 acceptance criteria

1. The type checker and LLVM backend consume the same validated foreign signature metadata.
2. Calls with the wrong arity fail before LLVM emission.
3. Incompatible fixed-width types are diagnosed rather than silently accepted as equivalent.
4. Integer literals outside the declared C parameter range fail with a source-located diagnostic.
5. A `void` foreign call is accepted as a statement but rejected when used where a value is required.
6. Existing `i32` integration (`fusion_helper(21)` prints `42`) continues to pass.
7. The full Rust test suite and Windows acceptance test pass after the change.

## Explicit non-goals for Phase 4.5.1

- No automatic pointer ownership or freeing.
- No C strings, slices, aggregates, variadics, callbacks, C++ ABI, or non-C calling conventions.
- No claim of runtime checking for arbitrary dynamic numeric narrowing until explicit runtime checks are implemented.
