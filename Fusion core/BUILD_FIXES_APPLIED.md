# Fusion core compile-error repair notes

Applied to the supplied Phase 3 source archive:

- Made the module parser mutable before calling its mutating `parse` method.
- Corrected type-provider re-export filtering to use the requested `symbol`.
- Kept pattern validation in the pattern-collection pass instead of passing a field-access vector to a pattern-output function.
- Cloned the receiver type before storing it in both resolved-call fields.
- Passed the nominal-name set to the ABI `layout_type` helper.
- Reworked JSON literal dispatch and TOML array-of-tables lookup to avoid mutable-borrow conflicts.
- Added the `sha2` and `hex` dependencies used by package-interface hashing and string encoding.
- Added explicit borrows for owned struct type names passed to `method_signature`.

Validation limitation: this environment does not have `cargo`, `rustc`, or `rustfmt` installed, so the repaired source could not be compiled or formatted here. Please run `cargo fmt` and `cargo test` in the Fusion core directory on your Windows machine and share any remaining diagnostics.
