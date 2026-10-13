# C Interoperability Plugin

C is Fusion's first serious interoperability target.

C is especially important because it provides a relatively stable native
interoperability boundary and is the foundation of a large amount of existing
systems software.

The goal is for installing C support to feel similar to installing a normal
Fusion dependency.

For example:

    fusion plugin install c

A Fusion project should eventually be able to declare a C dependency and have
Foundry handle the required compiler, linker inputs, headers, libraries, and
generated bindings.

## ABI support

Fusion needs to understand the important parts of a C ABI, including:

- calling conventions
- symbol names
- static libraries
- shared libraries
- primitive types
- structs
- unions
- enums
- pointers
- function pointers
- callbacks
- alignment
- platform-specific ABI differences
- compiler/linker arguments
- target triples

## Binding generation

A future C binding generator should be able to consume C headers or an
explicit interoperability description and generate Fusion declarations.

For example:

    C header
       |
       v
    C binding generator
       |
       v
    Fusion declarations
       |
       v
    Fusion program

The generator must not blindly assume that every C type is safe to expose.

It needs to understand:

- ownership
- nullability
- allocation
- deallocation
- pointer validity
- buffer lengths
- callback lifetime
- thread safety
- ABI layout

## Memory boundary

The memory boundary is one of the most important parts of C interoperability.

Fusion's memory management model must not be confused with the memory model of
a foreign C library.

For example, a C API might require:

    foo_create()
    foo_destroy()

Fusion should preserve that ownership contract instead of assuming that its
garbage collector owns the C object.

Likewise, Fusion should not automatically free memory allocated by a foreign
allocator unless the API explicitly permits it.

## Static and shared libraries

The plugin should support both:

- static libraries
- shared libraries

The plugin should communicate the resulting artifacts to Foundry so the
toolchain orchestrator can construct the final build graph.

## Initial milestone

Start with a small manually defined C ABI fixture.

Then implement:

1. C compiler discovery.
2. C target detection.
3. Header discovery.
4. Basic C type mapping.
5. Binding generation.
6. Static-library linking.
7. Shared-library linking.
8. Callback support.
9. ABI/layout validation.
10. Cross-platform tests.

## Status

This directory is currently a scaffold.

The Rust code below defines the future plugin boundary but does not yet invoke
a C compiler or generate bindings.