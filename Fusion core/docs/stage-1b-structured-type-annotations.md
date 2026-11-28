# Stage 1B — Structured type annotations in the AST

Stage 1B introduced the shared `TypeExpr` grammar, nested generic/function/tuple/array
syntax, and reference syntax. Its first increment temporarily stored structured types in a
parallel `Program.type_annotations` index alongside legacy string fields.

That temporary sidecar has been removed in Stage 1B.3. See
[`stage-1b3-structured-ast-types.md`](stage-1b3-structured-ast-types.md) for the current
AST representation and remaining compatibility boundaries.

Reference types are parsed and retained structurally, but borrowing, aliasing and lifetime
semantics are not yet implemented.
