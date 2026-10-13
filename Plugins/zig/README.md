# Zig Interoperability Plugin

Zig is comparatively friendly to Fusion interoperability because it has strong
C interoperability and targets systems programming.

The initial integration should therefore prefer:

    Fusion -> C ABI -> Zig

for libraries that expose stable C-compatible interfaces.

## Zig-specific concepts

A richer integration may eventually need to understand:

- comptime
- allocators
- slices
- error unions
- optionals
- compile-time execution
- Zig-specific data layouts

These concepts should not automatically be treated as ordinary C values.

## Initial strategy

Prefer Zig libraries exposing a documented C ABI.

Later, a Zig-aware binding generator can provide richer integration.

## Status

Scaffold only.