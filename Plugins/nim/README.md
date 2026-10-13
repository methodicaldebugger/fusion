# Nim Interoperability Plugin

Nim can compile to native code and has C interoperability, making a C ABI
an attractive initial integration boundary.

A typical first integration could be:

    Nim library
        |
        v
    C-compatible interface
        |
        v
      Fusion

A richer Nim-aware integration could eventually understand Nim-specific
constructs.

Potential concerns include:

- strings
- sequences
- exceptions
- memory management
- generated interfaces
- compiler/runtime requirements

## Initial milestone

Validate a small Nim library compiled into a C-callable artifact.

## Status

Scaffold only.