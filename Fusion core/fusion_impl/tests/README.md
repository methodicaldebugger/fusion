# Native conformance

The existing Rust unit tests remain in `lib.rs` so the interpreter/type checker
suite is preserved. Add native tests here as soon as the Rust toolchain is
available:

1. compile the `.fusion` source with `fusion build`
2. execute it
3. compare stdout with the interpreter's expected output

The native backend must never silently accept an unsupported construct.
