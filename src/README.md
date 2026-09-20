
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
