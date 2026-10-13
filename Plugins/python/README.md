# Python Interoperability Plugin

Python interoperability can give Fusion access to a broad ecosystem for AI,
scientific computing, data processing, automation, and existing libraries.

Unlike a native C ABI integration, Python libraries commonly depend on the
Python runtime, Python object model, and package environment.

## Initial integration strategy

Prefer an explicit, managed CPython integration:

    Fusion -> Python bridge -> CPython runtime -> Python package

Foundry should eventually manage the selected Python runtime, virtual
environment, package dependencies, and reproducible build/run configuration.

The first version should support a deliberately small set of boundary types,
such as booleans, numbers, strings, and sequences. More complex Python objects
should remain opaque handles unless bindings define their conversion and
lifetime behavior.

## Python-specific concepts

A richer integration may need to account for:

- CPython runtime initialization and shutdown
- Python object reference counting
- The Global Interpreter Lock (GIL), where applicable
- Python exceptions and Fusion `Result` conversion
- Virtual environments and package resolution
- Conversion between Python objects and Fusion types
- Callbacks and asynchronous execution

These concepts should not be treated as ordinary native ABI values. Runtime
and object lifetimes must be explicit at the interoperability boundary.

## Future direction

A later Python-aware binding generator could read annotations or generated
metadata to provide more ergonomic, typed Fusion APIs. Process-based or
other runtime bridges may also be useful for isolation and dependency conflicts.

## Status

Scaffold only. No runtime embedding, binding generation, package management,
or invocation is implemented.
