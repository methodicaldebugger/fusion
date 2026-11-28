# Stage 1B.4 — Structured explicit generic arguments and HIR call types

## Scope

This increment removes the remaining string representation of explicit generic
arguments from call expressions and carries structured type syntax into HIR.
It also extends HIR's semantic type algebra to represent function and reference
types without claiming that reference borrowing or native indirect calls are
implemented.

## AST and parser

- `Expression::Call.generic_arguments` and
  `Expression::MethodCall.generic_arguments` now contain `TypeAnnotation`
  values rather than `String` values.
- The parser constructs these annotations directly from `TypeExpr`.
- Explicit call-site type arguments deliberately bypass declaration generic
  erasure. In particular, `relay<T>(x) { identity<T>(x) }` retains `T` as a
  type variable instead of rewriting it to `unknown`.
- Generic-call lookahead now balances nested angle brackets, so nested types,
  arrays, references and function types can appear in explicit arguments while
  ordinary comparison expressions remain distinguishable by the following `(`.

## Type checking and interpreter compatibility

- Explicit generic arguments are converted from their `TypeExpr` trees by the
  type checker; they are no longer reparsed from strings.
- The checker tracks the generic parameters in scope for the function body, so
  an enclosing generic parameter can be forwarded to another generic function.
- The interpreter continues to treat generic arguments as compile-time-only
  metadata. Its call interface accepts the structured annotation values but
  does not need runtime type arguments.

## HIR

- HIR call and method-call expressions now retain `TypeExpr` generic arguments.
- HIR conversion expressions retain a `TypeExpr` target rather than a string.
- `HirResolvedCall` records the explicit generic argument trees separately from
  its semantic `HirType` signature and inferred/substituted return type.
- `HirType` now has structural `Function` and `Reference` variants. Resolution,
  generic unification and generic substitution recurse through both variants.
- Unresolved bare names in explicit generic arguments are provisionally
  represented as generic variables in the HIR call-resolution path. The source
  type checker remains responsible for confirming that such variables are in
  scope; unknown nominal constructors still fail HIR resolution.

## Boundaries

- HIR function/parameter/local/field declaration annotations still have some
  legacy string fields. Their migration is the next HIR slice.
- Reference syntax and HIR representation do not implement borrow checking,
  aliasing rules or lifetimes.
- HIR function types preserve signatures, but native indirect-call lowering is
  still a later callable-backend stage.
- User-defined generic type arguments are preserved, but complete generic
  declaration semantics, bounds and substitution are not implemented here.

## Regression coverage

- Parser test for nested explicit generic arguments and forwarding a caller's
  generic parameter.
- HIR lowering test for nested generic/function type arguments.
- HIR type-resolution tests for function and reference types.
- HIR structural inference test for an explicit generic return type.
- Integration test exercises generic forwarding in both single-file and
  project compilation paths.
