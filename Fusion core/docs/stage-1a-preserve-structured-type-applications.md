# Stage 1A — Preserve structured type applications

## Scope

This is the first compatibility-preserving step toward making `TypeExpr` the AST's actual type
annotation representation. It deliberately does not migrate every AST field from `String` to
`TypeExpr`; doing that safely requires coordinated changes across parser construction, AST
consumers, HIR, type checking, interpreter metadata, and backend interfaces.

## Implemented

- The existing `TypeExpr` grammar remains the single parser for annotations.
- Declaration-local generic parameter names are still lowered to `unknown` for compatibility.
- Generic arguments on user-defined nominal types are no longer discarded. For example,
  `Stack<Option<T>>[]` is retained as `Stack<Option<unknown>>[]` rather than `Stack[]`.
- The compatibility type checker resolves a user-defined generic constructor to its nominal
  struct/enum type while explicitly deferring arity validation and argument substitution.
- HIR project type validation accepts user-defined generic constructors and recursively validates
  their arguments. Built-in generic constructors retain their existing arity checks.
- Runtime parameter checks compare the outer nominal constructor for user-defined generic
  struct/enum annotations. Runtime `Struct`/`Enum` values do not yet retain applied type
  arguments, so concrete generic argument validation remains the responsibility of static
  checking.
- HIR call unification tolerates the current information-loss case where a local value has a
  nominal HIR type but the parameter has an applied generic type, provided both refer to the
  same nominal constructor. When both sides retain applied arguments, they are still recursively
  checked.
- Tests cover recursive preservation and a nested user-generic application through both the
  single-file and project compilation paths.

## Not implemented yet

- AST fields are still legacy `String` / `Option<String>` fields. This patch preserves the canonical
  structural spelling, but does not yet store `TypeExpr` directly in `ast.rs`.
- Generic parameters are not yet retained on struct, enum, or impl AST declarations as semantic
  parameters.
- User generic arity checking, type substitution, monomorphization, and generic native ABI support
  remain future stages.
- Reference types are not introduced by this patch.

## Validation

Run:

```powershell
cargo test --test generic_types
cargo test
```

A GitHub Actions workflow is included to run `cargo test` on pushes and pull requests. The patch was
source-reviewed and packaged in an environment without `cargo` or `rustc`; no test pass is claimed.
