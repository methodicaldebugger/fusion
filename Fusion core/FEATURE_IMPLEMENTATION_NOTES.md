# Fusion language feature implementation notes

This update extends the existing implementation in `lib.rs` and the compiler/interpreter modules.

## Implemented in this update

- Nested tuple type parsing and runtime tuple parameter validation/indexing.
- Nested enum payload patterns, tuple patterns, struct patterns (including `..`), and `name @ pattern` bindings.
- Recursive binding collection for pattern scopes.
- Type checking of struct fields in patterns, complete-field/rest validation, duplicate-field checks, and compatible binding checks for or-patterns.
- Fixed-width numeric type names: `i8`, `i16`, `i32`, `i64`, `i128`, `u8`, `u16`, `u32`, `u64`, `u128`, `isize`, `usize`, `f32`, and `f64`.
- Static checks for out-of-range integer literals in annotated variables, constants, and function returns; interpreter-side range checks and explicit numeric casts.
- Struct operator dispatch for `Add`-style `add`, `Index`-style `index`, and `Deref`-style `deref` methods in the type checker/interpreter. Derived `PartialEq` supports `==` and `!=` through `eq`.
- Regression tests added to `lib.rs` for nested enum patterns, struct patterns, `@` bindings, tuple types, fixed-width types/casts, and operator dispatch.

Existing support for match guards, inclusive/exclusive integer ranges, or-patterns, and `Debug`/`Clone`/`PartialEq` derives is retained, with the existing tests in `lib.rs`.

## Known limitations

- Exact-width integers (interpreter and type checker): integers that fit `i64` are still stored as `Value::Number(i64)`, and every other integer is stored as `Value::Wide(WideInt)` (see `wide_int.rs`), an exact sign-and-magnitude integer covering `i128::MIN ..= u128::MAX`. `u64` values above `i64::MAX`, and all `i128` / `u128` values, are therefore representable. `Value::from_wide` keeps the representation canonical, so equal integers always compare equal. Details:
  - The lexer produces `Token::WideNum(u128)` for literals above `i64::MAX` (up to `u128::MAX`); the parser/AST/HIR carry them as `Expression::WideNumber` / `HirExpression::WideNumber`. Negative literals use unary minus, so `-9223372036854775808`, `-170141183460469231731687303715884105728` and similar literals work.
  - Literal range checks are exact for every fixed-width type (`IntKind::contains`), including `u64` up to `18446744073709551615` and `u128` up to its maximum.
  - `+ - * /` and comparisons use `i64` arithmetic first and promote to exact wide arithmetic on overflow instead of wrapping or aborting; results outside `i128::MIN ..= u128::MAX` panic with "Integer overflow".
  - `as` casts, string parsing and float conversions are exact across the full range; integer to `f32` casts now round to single precision.
  - Runtime parameter, element and return checks now understand all fixed-width integer and float types (previously a parameter such as `value: i32` was rejected at runtime).
- Still not done: the native LLVM backend continues to lower every integer as `i64` (and `f32` as `double`). It now rejects wide literals with an explicit "LLVM backend" error instead of truncating them, but exact ABI-width emission (`i8` ... `i128`, signed vs unsigned division/comparison, `float` for `f32`) is future work. The `num` type remains an `i64`, so a value above `i64::MAX` cannot be passed to a `num` parameter. Match patterns still take `i64` literals only, and arithmetic is not wrapped or range-checked per static type except where a value meets a typed boundary (declaration, assignment, parameter, return). `f32` literals are kept at `f64` precision until they are cast.
- Operator overload dispatch is implemented for interpreter execution and static checking; native LLVM lowering for custom struct operators and dereferencing remains future work.
- Derives currently cover structs and the built-ins `Debug`, `Clone`, and `PartialEq`; this is not a general procedural-macro/attribute system.
- The test suite, including the new `wide_integer_tests` module in `lib.rs`, could not be compiled or run in this environment because `cargo` and `rustc` are unavailable. Run `cargo fmt --check` and `cargo test --lib` in a Rust development environment before merging.

## Phase 2 progress — native collection runtime

The native LLVM backend now lowers array literals through the existing tracked heap-allocation path used for tuples. Arrays retain their element type in backend state, and native indexing can use a runtime integer index. Array indexing emits a length check and traps on out-of-bounds access before loading an element. A small `fusion_tuple_len` runtime helper reads the collection length from the allocation header. Regression tests were added to `lib.rs` for array allocation, dynamic indexing, and bounds-check IR.

This is a partial Phase 2 milestone, not completion of the native runtime. The current storage format is an untagged sequence of 64-bit encoded values, so arrays of mixed types are prevented by the type checker but exact ABI-width values, nested ownership, mutation, garbage collection, closures, cleanup-aware `defer`, and complete native match/loop-control lowering remain unfinished. Native `for-each` lowering has an initial implementation for known array element types, but the parser currently does not expose a corresponding source construct, and control transfer from inside that loop is explicitly rejected rather than emitted incorrectly.

Build verification is still required in a Rust environment because `cargo` and `rustc` are unavailable in this execution environment.

## 0.3 standard library, test runner and doc generator

See `docs/stdlib.md`. New Rust modules: `text_ext`, `data_ext`, `crypto_ext`, `time_ext`,
`net_ext`, `os_ext`, `conc_ext` (pure `std`, each with unit tests) and the glue in `stdlib_ext`.
Integration points: one fall-through arm in `Interpreter::call_function`, one `extend` in the
type checker's builtin table, `#[test]`-style attributes in the parser, `///` comments in the
lexer, and the `test` / `bench` / `doc` commands in `main.rs`.

Status: written without access to a Rust compiler. Run `cargo test` in `Fusion core` first and
fix any compile errors before relying on it.


## Claude stdlib integration merge

Additive modules: `tz_ext.rs`, `tls_ext.rs`, and `h2_ext.rs`. Expanded `net_ext.rs`, `conc_ext.rs`,
`time_ext.rs`, and `stdlib_ext.rs` provide named zones, optional TLS/HTTP2, work-stealing tasks,
structured scopes, and optional concurrent async. Foundry's existing package graph and dependency
source fingerprinting were retained while test and API-doc orchestration was added.

No Rust build was available during the merge; run the default and optional-feature test matrices.
