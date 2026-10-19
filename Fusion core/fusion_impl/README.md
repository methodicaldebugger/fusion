
# Fusion bootstrap compiler

This tree turns the existing Fusion front-end into a small bootstrap-oriented
compiler pipeline.

## Pipeline

```text
Fusion source
  -> lexer
  -> parser
  -> name resolver
  -> type checker
  -> AST
  -> textual LLVM IR
  -> clang/LLVM
  -> native executable
```

The interpreter remains available for fast semantic testing.

## Commands

```text
fusion run program.fusion
fusion check program.fusion
fusion emit-llvm program.fusion -o program.ll
fusion build program.fusion -o program
```

The LLVM backend deliberately uses textual LLVM IR instead of an LLVM Rust
binding in this bootstrap stage. That keeps the compiler self-contained and
avoids binding/version coupling. Once the language is capable of compiling
itself, the textual emitter can be replaced or supplemented by `inkwell` or
`llvm-sys` without changing the language front-end.

## Bootstrap strategy

1. Keep the lexer/parser/type checker authoritative.
2. Resolve names before code generation.
3. Keep the interpreter as the executable semantic oracle.
4. Lower a well-defined subset to LLVM.
5. Use conformance tests to compare interpreter and compiled behavior.
6. Add collection/runtime/foreign-ABI lowering as explicit compiler
   subsystems rather than leaking them into the parser.

The current backend covers scalar literals, locals, functions, calls, printing,
arithmetic, comparisons, boolean operations, if/while/for and returns.
Collections, pattern matching, defer and foreign calls are intentionally
reported as backend gaps rather than silently miscompiled.

## 0.2 standard-library runtime additions

The interpreter now exposes these built-ins without external Rust dependencies:

- Async/task syntax: `async fn`, `await`
- Filesystem: `read_file`, `write_file`, `file_exists`, `remove_file`, `create_dir`, `list_dir`
- CLI/system: `args`, `process_run`, `system`, `cwd`, `env_get`
- Time: `sleep_ms`, `now_ms`, `unix_time`, `date_utc`
- Randomness: `random_int`, `random_float`
- Networking: `tcp_connect`, `TcpStream.send/receive/close`
- Serialization: `serialize`, `deserialize` (primitive/array subset)
- Formatting: `format(template, values)` with `{0}`, `{1}`, ... placeholders
- Encoding: `hex_encode/decode`, `base64_encode/decode`
- Logging: `log_debug`, `log_info`, `log_warn`, `log_error`

These are intentionally interpreter-first implementations. The compiler/IR architecture remains independent of the host runtime details so the same APIs can later be backed by a native Fusion runtime.


We need to make one integration so good that it proves the entire model.
1. Milestone 1 — C plugin: Compile a real .c file, link a C library, and call it from Fusion.
2. Milestone 2 — Foundry toolchain management: Automatically obtain a pinned C compiler and linker, isolate the environment, and cache downloads.
3. Milestone 3 — Reliable boundaries: Generate or maintain bindings, validate types, handle errors, and document memory and resource ownership at the ABI boundary.
4. Milestone 4 — Second integration strategy: Add a language requiring a different mechanism, such as Python through an embedded runtime or process bridge.
5. Milestone 5 — Integration SDK: Make it practical for the community to develop new language plugins without modifying Fusion's compiler.