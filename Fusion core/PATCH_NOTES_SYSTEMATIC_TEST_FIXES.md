# Fusion systematic test-failure fixes

This patch is based on the uploaded `fusion – full copy.zip` source snapshot.

## Source fixes

- Parse `let name = expression` as an inferred variable declaration (`declared_type: None`) rather than treating `let` as a user-defined type name.
- Detect indentation/braces block style across multiline declaration headers, including aligned closing-parenthesis/return-type lines and a delimiter on its own line.
- Resolve re-exported function providers using the original source-name index (`ModuleUnit::function_symbols`), which remains valid after compatibility-AST function names are mangled.
- Use the interpreter's recursive value/type matcher for function parameter validation. The previous duplicate parameter checker did not support tuple parameters, including nested tuple types.
- Remove a duplicate LLVM branch terminator in direct HIR `Goto` emission.

## Regression-test corrections/additions

- Add parser coverage for inferred `let` declarations.
- Add lexer coverage for multiline function signatures.
- Correct LLVM runtime-helper assertions to match the shared cross-package runtime ABI (external helper definitions, not `internal`).
- Correct the foreach HIR test: its loop body returns immediately, so it must not expect a branch to the increment/step block after the return terminator.
- Match the emitted heap allocator signature including its named `%bytes` parameter.
- Correct the package ABI test fixture so `hidden_inference` is actually private, matching its expected `abi-internal-unsupported` record.

## Validation status

The archive and source edits were inspected and ZIP integrity is checked. This environment does not contain `cargo`, `rustc`, or `rustfmt`; therefore the Rust test suite and formatter could not be run here. Run `cargo fmt -- --check` and `cargo test` from `Fusion core/` after extracting this archive. Do not treat this patch as a verified passing build until those commands complete successfully.
