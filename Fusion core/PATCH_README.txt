Fusion Stage 2 corrective patch — standard-library builtin name resolution

Cause
-----
Stage 2 added first-class standard-library builtin handling to the type checker
and interpreter, but the name resolver still treated a bare builtin identifier
(e.g. `str_char_count`) as an unknown variable. This stopped compilation before
callable-value checking could run.

Fix
---
name_resolution.rs now accepts names registered by
`crate::stdlib_ext::is_builtin` in both:
- ordinary identifier expressions; and
- closure capture metadata.

No other source files are changed. In particular, this patch does not replace
foreign.rs, type_checker.rs, the Phase 4.5.2 pointer-contract documentation, or
any LLVM backend files.

Apply
-----
Extract this ZIP into the existing Fusion core directory and overwrite
name_resolution.rs.

Validate (PowerShell)
--------------------
cargo test --test function_values
cargo test
