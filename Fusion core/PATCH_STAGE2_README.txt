Fusion — Stage 2 patch: generic functions and standard-library builtins as values

BASELINE
Apply this patch on top of the Stage 1 interpreter-closures patch that you confirmed
with a passing `cargo test` run. Do not apply it to the pre-Stage-1 archive.

FILES
- value.rs: adds the Builtin callable value variant and display/equality behavior.
- interpreter.rs: evaluates stdlib builtins as values, invokes builtin values, accepts
  them in function-typed parameters, and supports builtin captures in Stage 1 closures.
- type_checker.rs: exposes generic function signatures as values, exposes registered
  stdlib builtin signatures as values, infers generic substitutions at indirect calls,
  and checks generic callables against concrete function types.
- stdlib_ext.rs: keeps builtin function values non-serializable.
- tests/function_values.rs: regression coverage for generic functions and builtin values.
- docs/stage-2-generic-functions-and-builtin-values.md: design, scope, and limitations.

APPLY
Extract the ZIP into the existing `Fusion core` directory and allow the files above
to overwrite their matching files. The patch intentionally does not include foreign.rs,
 type_checker.rs from the old Phase 4.5.2 patch, or any LLVM backend changes; the current
 type_checker.rs is an incremental update and retains the Stage 4.5.2 pointer contracts.

VALIDATE (PowerShell)
  cargo test --test function_values
  cargo test

IMPORTANT SCOPE NOTES
- First-class builtin values are currently limited to names registered by
  `stdlib_ext::is_builtin` / `stdlib_ext::signatures()`.
- Core direct-call-only operations such as print/input/open are not yet first-class.
- Generic function values infer type arguments from indirect-call arguments. No new
  standalone explicit-specialization expression syntax is introduced in this stage.
- This patch does not implement LLVM indirect calls or native closure environments.

The authoring environment did not have cargo/rustc, so no Rust compilation or test run
is claimed here. Please report the first compiler/test failure verbatim if one occurs.
