# Phase 3 module boundary hardening


## Follow-up correctness fixes (2026-11-14)

- Removed the duplicate HIR generic-argument splitter by naming the owned-string variant `split_hir_type_arguments_owned` and updating its call sites.
- Corrected module integration tests to use `Vec<String>::iter().any(...)` and updated the symlink diagnostic expectation to the package-source-root wording.
- Corrected escaped newline sequences in the three-level selective-re-export regression fixture.
- Module lookup now prefers importer-relative ancestor namespaces before crate-root names and no longer uses a global suffix fallback. This fixes sibling shadowing and avoids false ambiguity between unrelated packages.
- Selective re-export traversal now follows only edges that actually select the requested symbol. Explicitly selected missing/private names produce diagnostics instead of being silently dropped.
- Foundry now passes the resolved dependency graph's complete alias/root set to the compiler, enabling a dependency package to import its own transitive dependency. Conflicting transitive aliases are rejected explicitly rather than silently binding to an arbitrary package.
- Formatting is not verified in this environment because `cargo`/`rustfmt` are unavailable. Run `cargo fmt --all` before CI. The Rust test suite likewise remains unexecuted here.

## Remaining limitations

- Compiler package roots are still represented as a flat alias map. If two different branches of the dependency graph use the same alias for different packages, Foundry now reports a clear error. A future package-scoped import-root table is needed to support that valid graph without alias renaming.
- Path dependencies' own Git/registry dependencies still depend on Foundry materializing those caches during recursive dependency resolution; this pass does not claim to have fixed that earlier cache issue.
- `package_graph` entry selection (`main.fusion`) and compiler package entry selection (`lib.fusion`) still need to be unified.
- `foundry build --target` forwarding remains a separate CLI/compiler contract issue and is not changed in this pass.
