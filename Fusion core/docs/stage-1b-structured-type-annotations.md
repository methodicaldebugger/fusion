# Stage 1B — Structured type annotations in the AST

## What this patch establishes

- Adds `StructuredTypeAnnotation` and `TypeAnnotationRole` to the AST.
- `Program` now carries a source-spanned index of structured `TypeExpr` trees for
  parameter, return, local, constant, struct-field, enum-payload, trait-method,
  and impl-target annotations.
- The parser rebuilds this index after derive/default-method expansion. Project
  loading and namespace lowering also rebuild it so tooling does not observe stale
  names.
- Adds `&T` and `&mut T` to the shared type-expression grammar and lexer/parser.
  Parenthesized reference types can be used as array element types.
- HIR function-type erasure recursively preserves reference syntax.

## Compatibility boundary

This is the first AST migration slice, not the final removal of legacy strings. The
existing `Option<String>` and `String` annotation fields remain as a compatibility
projection because the type checker, interpreter, HIR lowering, module validation,
and LLVM backend still consume them in several places. The new AST sidecar gives
those passes and tooling a structured representation without forcing an unsafe
all-at-once rewrite. Generic declaration parameters continue to be erased to
`unknown` in legacy spellings where the current parser already did so.

Reference types are parsed and retained structurally, but the semantic type system
and runtime do not yet implement borrowing. Type checking therefore reports an
explicit unsupported-reference diagnostic rather than pretending references have
value semantics.

## Verification

Added parser/type-expression tests for nested generic + function types, immutable
and mutable references, and arrays of references. Run:

```powershell
cargo test type_expr
cargo test ast_retains_structured_nested_function_and_reference_types
cargo test
```

The patch authoring environment did not have Cargo/rustc available, so these tests
require verification on a Rust-enabled machine or GitHub Actions.
