# Dart Interoperability Plugin

Dart provides native interoperability through FFI, but a complete integration
must also understand the Dart runtime.

Important concepts include:

- Dart VM
- JIT
- AOT
- isolates
- garbage collection
- asynchronous execution
- callbacks
- object representation
- FFI

## Initial strategy

Prefer a stable native interface:

    Fusion
       |
       v
    Native ABI
       |
       v
    Dart FFI adapter
       |
       v
      Dart

The integration should avoid exposing arbitrary Dart objects directly across
the boundary.

## Runtime modes

Dart's execution environment matters.

The plugin should explicitly distinguish:

- JIT
- AOT
- standalone runtime
- platform-specific embedding

## Initial milestone

Create a small Dart FFI adapter and document its runtime assumptions.

## Status

Scaffold only.