We could eventually have:

C header
C++ headers
Rust metadata
.NET assembly
Java JAR
Swift module
Go package
Zig package
Nim module
│
▼
Universal Binding Generator
│
▼
Fusion API

the generator should recognize: 
Can represent directly? -> yes -> Generate binding 
no -> Generate adapter 
no -> Runtime bridge 
no -> RPC/WASM 
no -> Unsupported/manual wrapper

We shouldn't need to build a bespoke integration for every language on Earth.

For an unsupported language, Fusion could say:

Fusion package:

my_library
backend: external
transport: wasm

or:
transport: rpc

or:
transport: c_abi

or:
transport: generated_binding

So someone could create:
Fusion ↔ Haskell
Fusion ↔ Julia
Fusion ↔ Lua
Fusion ↔ Ruby
Fusion ↔ PHP
Fusion ↔ D
Fusion ↔ Fortran
Fusion ↔ OCaml
Fusion ↔ Elixir
...

without the Fusion core compiler needing to understand the entire language.

This leads to a very important architectural principle
Fusion should not contain all integrations.

There will be a downloadable adapter/plugin system for every new language.
That way the community can build integrations without modifying the Fusion compiler every time.

Fusion's package manager should ideally make foreign dependencies feel like Fusion dependencies.
We're trying to create: "An interoperability platform with a language on top of it."
The interoperability layer makes the platform valuable.
And the adapters/bridges allow the platform to grow without requiring the Fusion core team to personally implement every programming ecosystem in existence.