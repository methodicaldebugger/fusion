# Phase 4.1: Foundry C source build integration

Foundry now treats project-local C source files as native link inputs.

## Usage

Place C translation units under the project root's `c/` directory. Foundry
recursively discovers `.c` files there and compiles them to cached object files
in `target/<profile>/foreign-c/` (or the selected target subdirectory).

Example layout:

```
my-project/
  Foundry.toml
  src/
    main.fusion
  c/
    helpers.c
    math/
      operations.c
```

A C compiler is required when this directory contains C sources. Foundry checks
`CC` first, then searches for Clang or GCC (and `cc` on non-Windows). The C
compiler is invoked with `-c` and output-object arguments. Compiled objects are
passed to `fusion link-llvm` alongside Fusion LLVM artifacts.

C file names and contents participate in the executable build fingerprint.
Changing, adding, or removing a C translation unit invalidates the cached Fusion
executable. Object files are cached by source identity.

## Scope and limitations

This is build/link integration only. Fusion language-level foreign declarations,
ABI type checking, calls into C, header binding generation, static/dynamic library
manifest declarations, and ownership rules are not implemented in this step.

The initial compiler driver contract is Clang/GCC-style (`-c`, `-o`). Windows
MSVC-only `cl.exe` is not currently supported by this implementation.

## Build configuration

Set `CC` to select the compiler executable (either a path or a command found on
`PATH`). Set `FOUNDRY_CFLAGS` to additional compiler flags; the initial
implementation tokenizes this value on whitespace, so quoted multi-word flag
values are not supported. Compiler path, reported version, flags, and C source
content/path are included in the C object/build identity. This is a Clang/GCC
style driver interface, not an MSVC `cl.exe` interface.
