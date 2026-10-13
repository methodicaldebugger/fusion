# Rust Interoperability Plugin

Rust interoperability is more complicated than C because Rust's type system
contains concepts that do not map directly onto Fusion.

These include:

- ownership
- borrowing
- lifetimes
- traits
- generics
- async
- panics

Fusion should not attempt to expose Rust's ownership and lifetime system
directly as though Fusion itself had become Rust.

## Proposed boundary

A Rust library can explicitly expose a Fusion-compatible interface.

For example:

    Rust library
         |
         v
    Fusion-compatible interface
         |
         v
    Generated wrapper
         |
         v
       Fusion

A C ABI can also be used where appropriate.

## Ownership

The boundary must explicitly describe whether Fusion:

- owns an object
- borrows an object
- receives a copied value
- receives an opaque handle

Borrowed Rust references should never silently become arbitrary long-lived
Fusion values.

## Panics

Rust panics must have an explicit policy.

A future plugin may require that exported functions:

- never panic
- catch panics at the boundary
- translate failures into Fusion Result values

## Initial milestone

Start with `repr(C)` data and `extern "C"` functions.

After that, develop generated wrappers for explicitly declared
Fusion-compatible APIs.

## Status

Scaffold only.