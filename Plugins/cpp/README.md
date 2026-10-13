# C++ Interoperability Plugin

C++ interoperability is more complicated than C because C++ does not provide
one universally stable ABI across all compilers, standard libraries, versions,
and platforms.

Fusion should therefore not attempt to expose every C++ feature directly.

## Initial strategy

Prefer:

    C++ library
        |
        v
    Stable C-compatible wrapper
        |
        v
    Fusion binding

A second strategy can eventually be:

    C++ headers
        |
        v
    C++ binding generator
        |
        v
    Fusion wrapper

## What should be handled carefully

C++ integration eventually needs to account for:

- classes
- constructors/destructors
- templates
- overloaded functions
- namespaces
- exceptions
- RTTI
- virtual functions
- object lifetime
- name mangling
- ABI compatibility
- standard-library types
- compiler-specific behavior

Fusion should not pretend these are equivalent to ordinary C functions.

## Stable C APIs

A C-compatible wrapper is often the safest boundary.

For example:

    C++ implementation
          |
          v
    extern "C" wrapper
          |
          v
        Fusion

This lets the C++ library preserve its internal implementation while
providing a stable interface to Fusion.

## Initial milestone

Start with C-compatible wrappers.

Only introduce direct C++ binding generation after the ABI, lifetime, exception,
and type-conversion rules are well defined.

## Status

Scaffold only.