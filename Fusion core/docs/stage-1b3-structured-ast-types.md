# Stage 1B.3 — Structured types are authoritative in the AST

## Implemented

- Replaced source annotation fields that used `Option<String>` / `String` with
  `TypeAnnotation`, whose authoritative payload is a `TypeExpr` tree:
  - function and trait-method parameter/return annotations;
  - local and constant declarations;
  - struct fields and enum payload types;
  - impl receiver type;
  - explicit conversion target types.
- Removed `Program.type_annotations`, `StructuredTypeAnnotation`,
  `TypeAnnotationRole`, and the rebuild/indexing passes. Type information now
  lives on the AST node that owns the annotation.
- The parser constructs `TypeAnnotation` directly from its parsed `TypeExpr`.
  Generic declaration parameters continue to be normalized to `unknown` inside
  their declarations until generic user-type semantics are implemented; nested
  generic/container/function structure is retained.
- The type checker converts AST annotations directly from `TypeAnnotation.expression`.
  String parsing remains only for compatibility boundaries that still accept
  standalone spellings (for example HIR metadata and explicit generic call arguments).
- Module namespace rewriting and source-reference analysis now traverse structured
  `TypeExpr` nodes for AST annotations instead of editing/scanning their display strings.
- Trait default expansion substitutes `Self` structurally.
- HIR lowering converts structured annotations to canonical display spellings at the
  current HIR compatibility boundary. HIR's own type fields remain strings in this
  increment; they are not source AST annotations.

## Compatibility and limitations

- `TypeAnnotation` retains a private canonical display cache for existing diagnostics,
  interpreter checks and HIR/backend boundaries. The type tree is authoritative; the
  cache is refreshed whenever namespace rewriting changes the tree.
- Explicit type arguments on call and method-call expressions remain `Vec<String>`
  for now. They are the next small AST migration surface; the generic-call checker and
  HIR project passes still consume their canonical spellings.
- Reference syntax is retained structurally but borrowing, aliasing and lifetime
  semantics are not implemented. The checker continues to reject reference types
  semantically.
- Generic user-type parameters, substitution and trait bounds remain a later feature.

## Regression coverage

- Parser test asserts nested generic/function/reference types are present directly on
  parameter and return AST nodes.
- Type checker test converts an AST `TypeAnnotation` directly without parsing a spelling.
- AST test verifies module-name rewriting traverses nested function, array, generic and
  reference type nodes.

Run:

```powershell
cargo test type_expr
cargo test generic_types
cargo test modules
cargo test function_values
cargo test
```

The patching environment has no Rust/Cargo toolchain, so compilation and tests must be
verified on the developer machine or GitHub Actions.
