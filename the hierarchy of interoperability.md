I would not build ten completely independent systems.

Instead, create something like:

Fusion Interop -> Native ABI -> C/Zig/etc.
            or -> Runtime Bridge -> JVM/.NET/etc
            or -> High-level Bridge -> Rust/C++/Swift
                         
                    Fusion Interop
                         │
          ┌──────────────┼───────────────┐
          │              │               │
       Native          Runtime        High-level
        ABI             Bridge          Bridge
          │              │               │
      C/Zig/etc.      JVM/.NET/etc.   Rust/C++/Swift

And underneath that:

Level 1 — Native ABI
Fusion ↔ C ABI ↔ native library

Useful for:

C
Zig
many C-compatible Rust libraries
many C-compatible C++ libraries
many Nim libraries

Level 2 — Generated bindings
Language API -> binding generator -> Fusion API

Useful for:

Rust
C++
Swift
Go
Nim
etc.

Level 3 — Runtime bridges
Fusion -> .NET runtime -> C#

Fusion -> JVM -> Java/Kotlin
Level 4 — Process/RPC bridges

When embedding the runtime isn't desirable:

Fusion process --RPC-> Other-language process

This can work for practically any language.

Level 5 — WebAssembly

Another universal escape hatch:

Other language -> WebAssembly -> Fusion

This is especially useful for languages/ecosystems that don't have a convenient native ABI.