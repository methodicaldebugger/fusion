# Stage 1B.2 — Structured type consumption (superseded by Stage 1B.3)

Stage 1B.2 migrated type checking and HIR type resolution to consume structured `TypeExpr`
syntax while it was still held in the temporary `Program.type_annotations` index.

Stage 1B.3 removed that sidecar and moved each structured annotation onto its owning AST
node. The current design and compatibility boundaries are documented in
[`stage-1b3-structured-ast-types.md`](stage-1b3-structured-ast-types.md).
