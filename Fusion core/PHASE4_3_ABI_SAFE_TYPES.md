# Phase 4.3 — ABI-safe C types

Phase 4.3 tightens the declaration contract established in Phase 4.2. It defines
which Fusion type spellings may cross the initial C ABI boundary and rejects
foreign declarations that require unsupported language/runtime behavior.

## Supported type matrix

| Fusion spelling | ABI category | Parameters | Returns |
|---|---|---:|---:|
| `i8`, `u8` | 8-bit integer | Yes | Yes |
| `i16`, `u16` | 16-bit integer | Yes | Yes |
| `i32`, `u32` | 32-bit integer | Yes | Yes |
| `i64`, `u64` | 64-bit integer | Yes | Yes |
| `f32`, `f64` | IEEE floating-point scalar | Yes | Yes |
| `*opaque` | Opaque raw pointer/handle | Yes | Yes |
| `void` | No value | No | Yes |

The `void` return is selected by omitting the return annotation or spelling
`-> void`. A `void` parameter is always rejected. The accepted integer types are
fixed-width; pointer width follows the target C ABI and is not represented as a
fixed-width integer.

## Rejected declarations

- Types outside the table, including `i128`, `u128`, `isize`, `usize`, `bool`,
  strings, and user-defined structs.
- Generic foreign functions.
- Asynchronous foreign functions.
- Foreign ABIs other than `C`.
- Foreign calls themselves remain unavailable until Phase 4.4 implements native
  call lowering.

## Pointer and ownership contract

`*opaque` is an uninterpreted pointer/handle. Fusion does not inspect the pointee,
perform automatic allocation or deallocation, or infer ownership transfer. The
caller remains responsible for validity, lifetime, aliasing, and any API-specific
release operation. This milestone does not add nullable/reference semantics or
safe wrapper generation.

## Boundary of this milestone

The type checker validates declaration types and rejects unsupported declaration
shapes. This does not yet prove that a foreign implementation matches its declared
signature, generate C header bindings, emit LLVM declarations, or perform a call.
Those are later build/link and Phase 4.4 lowering responsibilities. Target-specific
LLVM ABI details must be handled when lowering and linking for the selected target.

## Tests

Regression coverage includes every supported parameter type, representative
return types including `void` and `*opaque`, unsupported type spellings, generic
and async declarations, and the existing Phase 4.2 invocation diagnostic.
