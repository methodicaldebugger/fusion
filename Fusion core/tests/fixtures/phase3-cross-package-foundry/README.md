# Phase 3 cross-package Foundry fixture

This three-package workspace is used by `scripts/phase3-real-toolchain-smoke.sh`.

- `model` exports `Point` and a payload-bearing `Outcome` enum.
- `consumer` depends on `model`, accepts/returns those nominal types, and accesses a struct field.
- `app` depends on both packages, constructs values, matches enum variants, and prints the expected output:

```text
7
9
failed
```

The smoke script builds the actual Fusion and Foundry Rust binaries, runs the real Foundry package build/link pipeline, executes the native program, and verifies compiler/linker/package/executable hashes in Foundry's format-v5 provenance document. The script requires a working Rust toolchain, Clang, and Python 3.11+.
