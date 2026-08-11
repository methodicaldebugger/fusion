Rust interoperability is slightly harder than C

Rust library -> Fusion-compatible interface -> generated wrapper -> Fusion

A Rust library could explicitly expose an interoperability layer and Fusion tooling generates the necessary bridge.

Rust-specific concepts such as:

ownership
borrowing
lifetimes
traits
async
generics

would need to be translated into Fusion-compatible concepts, rather than exposing Rust's type system directly.