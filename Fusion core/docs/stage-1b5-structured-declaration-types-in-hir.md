# Stage 1B.5 — Structured declaration types throughout HIR

## Scope

Source-preserving HIR declaration types now retain `TypeExpr` trees rather than storing only
serialized type spellings. `HirTypeAnnotation` carries the structured expression as its
source of truth and a canonical display spelling for compatibility diagnostics and consumers
that have not yet migrated to structural traversal.

The migration covers:

- Function and trait-method parameter and return annotations.
- Local, uninitialized-local, and constant declarations.
- Struct field annotations.
- Enum payload annotations.
- Impl receiver/target annotations, including generic applications.
- The HIR nominal type registry's struct fields and enum payloads.

## Compatibility boundary

`HirTypeAnnotation` implements `Deref<Target = str>` and `Display` so existing diagnostics,
name-resolution helpers, and interpreter/backend compatibility paths can continue to consume
a canonical spelling. New semantic code should use `.expression` or `.as_type_expr()` and
must not parse the compatibility spelling back into a type tree.

The existing semantic `HirType` algebra remains the resolved representation. Converting a
source `TypeExpr` into a resolved `HirType` still requires module/type-registry context and is
therefore performed by the existing resolver rather than during syntax lowering.

## Non-goals

This stage does not add generic user-type substitution, reference/borrow semantics, new trait
semantics, or changes to interpreter/LLVM execution. It changes HIR declaration data shape
while retaining the current semantic behavior.

## Tests

Unit tests verify that nested generic, function, reference, array, and generic-parameter
shapes survive construction of `HirTypeAnnotation` without flattening.
