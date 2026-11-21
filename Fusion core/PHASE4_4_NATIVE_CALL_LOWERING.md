# Phase 4.4 — Native C Call Lowering

## Status

Implementation added for the initial Phase 4 C ABI subset. The Rust test suite and a Windows native acceptance run still need to be executed by the project owner; this environment does not have Cargo installed.

## Implemented path

- Foreign declarations remain ordinary top-level `extern "C"` function prototypes in the AST.
- The type checker now permits calls to validated foreign declarations and applies the normal fixed-arity and parameter-type checks.
- LLVM collection retains the exact `ForeignAbiType` signature separately from Fusion's native scalar representation.
- The backend emits external LLVM declarations using the C calling convention (`ccc`).
- Foreign calls target the declared native link symbol, including a distinct `link_name` when supplied.
- Module-name mangling preserves the original foreign symbol as the default link name before rewriting the Fusion-visible function name.

## ABI conversions

| C ABI type | LLVM type | Fusion-side value representation | Boundary operation |
|---|---|---|---|
| `i8`, `u8` | `i8` | `i64` | truncate on input; sign/zero extend on return |
| `i16`, `u16` | `i16` | `i64` | truncate on input; sign/zero extend on return |
| `i32`, `u32` | `i32` | `i64` | truncate on input; sign/zero extend on return |
| `i64`, `u64` | `i64` | `i64` | direct |
| `f32` | `float` | `double` | `fptrunc` on input; `fpext` on return |
| `f64` | `double` | `double` | direct |
| `*opaque` | `ptr` | `ptr` | direct |
| `void` return | `void` | no value | call with no result |

Integer narrowing follows LLVM truncation semantics at the ABI boundary. This milestone does not add checked numeric range conversions.

## Scope limits

The initial subset remains C-only and excludes structs/unions by value, variadics, callbacks, strings/slices, automatic ownership, C++ ABI, and non-C calling conventions. Opaque pointers are passed as raw handles; Fusion does not infer ownership or lifetime.

## Acceptance test

The expected end-to-end project test is:

```fusion
extern "C" {
    fn fusion_helper(value: i32) -> i32;
}

main:
    print(fusion_helper(21))
```

with the existing C helper:

```c
#include <stdint.h>

int32_t fusion_helper(int32_t value) {
    return value * 2;
}
```

Foundry must compile the C source, pass the resulting object to `fusion link-llvm`, and produce a native executable that prints `42`.
