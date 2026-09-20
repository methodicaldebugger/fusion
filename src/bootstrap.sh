#!/usr/bin/env sh
set -eu
cargo build --release
mkdir -p target/bootstrap
./target/release/fusion check examples/hello.fusion
./target/release/fusion emit-llvm examples/hello.fusion -o target/bootstrap/hello.ll
./target/release/fusion build examples/hello.fusion -o target/bootstrap/hello
./target/bootstrap/hello
