# Fusion Language Interoperability

This directory contains optional Foundry plugins for integrating Fusion with
existing programming-language ecosystems.

The goal is not to rewrite existing software.

A mature Fusion ecosystem should allow developers to keep substantial existing
codebases—including very large Java, C++, C#, C, Rust, and other systems—and
write new software in Fusion while consuming those existing systems.

For example:

- Existing C libraries remain C.
- Existing C++ libraries remain C++.
- Existing C# applications remain C#/.NET.
- Existing Java applications remain Java/JVM.
- Existing Rust libraries remain Rust.
- New application code can be written in Fusion.
- Fusion can consume selected capabilities from those ecosystems through
  explicit interoperability boundaries.

The objective is a unified development experience, not a claim that every
language or library can magically interoperate without adapters.

## Transferable knowledge

One of the larger goals of Fusion interoperability is to make knowledge
transferable across ecosystems.

A tutorial explaining how to use a particular database, protocol, algorithm,
API, or library should remain useful even when the application is written in
Fusion rather than the language used by the original tutorial.

Likewise, existing libraries should remain useful instead of requiring
developers to rewrite them in Fusion.

## Plugin model

Language integrations are optional Foundry plugins.

They should not be bundled into Fusion Basic.

A Fusion installation should contain the Fusion compiler/runtime, Foundry,
the interop model, and plugin management infrastructure. Individual language
integrations can be installed when a project needs them.

For example:

    fusion plugin install c

or a project could eventually declare a dependency such as:

    [plugins]
    c = "1.0"

Foundry should then:

1. Resolve the requested plugin.
2. Verify its version and source.
3. Verify that the required external toolchain is available.
4. Discover the compiler/linker/runtime required by the plugin.
5. Construct the cross-language build graph.
6. Generate bindings or adapters where necessary.
7. Build the foreign component.
8. Link, embed, or otherwise connect it to the Fusion program.

Plugins should never silently execute arbitrary downloaded code.

## Different interoperability mechanisms

Not every language should use the same integration mechanism.

Possible boundaries include:

- Native ABI
- C ABI
- Generated bindings
- Generated adapters
- Managed runtimes
- Embedded runtimes
- Foreign-language processes
- RPC
- WebAssembly
- Explicit serialization boundaries

The plugin should choose the appropriate mechanism for its ecosystem.

## Important principle

Foreign source code remains in its native ecosystem.

Fusion should not require developers to put C, C++, Rust, Java, C#, or other
foreign syntax directly inside `.fusion` files.

Instead:

    Fusion source
        |
        +--> generated bindings
        |
        +--> native library
        |
        +--> managed runtime
        |
        +--> foreign process
        |
        +--> WebAssembly module

The exact mechanism depends on the language.

## Current priorities

C should be the first serious interoperability target.

C# should be the second major target because .NET provides access to a huge
managed ecosystem and represents an important class of runtime-based
interoperability.

After those, Fusion can expand into:

- C++
- Rust
- Zig
- Nim
- Go
- Swift
- Java
- Dart
- Kotlin

Each ecosystem should be implemented incrementally rather than claiming
support before its runtime, memory model, build system, and failure modes are
understood.

## Plugin responsibilities

A language plugin should eventually describe:

- language identity
- compiler/toolchain requirements
- supported target triples
- runtime requirements
- ABI requirements
- build inputs
- generated artifacts
- linker requirements
- include paths
- library paths
- compiler arguments
- binding generation
- type conversions
- memory ownership
- allocation/deallocation
- callbacks
- error handling
- exception handling
- concurrency boundaries
- runtime lifecycle
- package-manager integration
- version compatibility
- security requirements
- diagnostics
- tests

The plugin should orchestrate existing ecosystem tools rather than attempting
to replace mature compilers and package managers.

## Architecture

The intended relationship is roughly:

    Fusion
       |
       v
    Foundry
       |
       v
    Language Plugin
       |
       +-------------------+
       |                   |
       v                   v
    Toolchain          Runtime / ABI
       |                   |
       +---------+---------+
                 |
                 v
          Foreign Artifact
                 |
                 v
             Fusion Program

The exact graph depends on the ecosystem.

## Status

These folders are currently architectural scaffolds.

The presence of a plugin directory does not mean Fusion already supports that
language.

A language should only be advertised as supported once its implementation,
toolchain discovery, interoperability boundary, and tests actually work.