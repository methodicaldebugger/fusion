# Fusion bootstrap plan

## Stage A — semantic bootstrap

The interpreter is the semantic oracle. Every language feature is covered by
conformance tests before code generation is considered correct.

## Stage B — native bootstrap

The compiler pipeline is:

`lexer -> parser -> resolver -> type checker -> LLVM emitter -> clang`.

The LLVM emitter uses textual LLVM IR in the first bootstrap generation. This
is intentional: it removes a dependency on a particular LLVM Rust binding and
makes the generated artifact inspectable with standard LLVM tools.

## Stage C — self hosting

Once Fusion can express the lexer, parser, resolver, type checker and IR
emitter, port those components to Fusion in this order:

1. spans/errors
2. lexer
3. AST
4. parser
5. type representation/checker
6. HIR/IR
7. LLVM text emitter
8. compiler driver

Build Fusion-compiler-N+1 with compiler-N, then compare the generated LLVM
and executable behavior. The first self-hosting compiler does not need to
replace every Rust implementation at once.

## Backend invariants

* No silent fallback for unsupported constructs.
* Interpreter and compiled execution are compared by conformance tests.
* Foreign code is project-level input, never embedded in a Fusion source file.
* LLVM IR is a stable backend boundary.
* Resource cleanup (`defer`) will eventually lower to explicit cleanup CFG.
* `match`, `break`, `continue`, async and collection operations require CFG or
  runtime support before being enabled in native code generation.

## Foreign C bootstrap

The first foreign-language boundary is the C ABI. The build system can compile
C sources with clang into objects and link them with Fusion output. A future
`foreign` declaration should produce a generated Fusion-facing binding, while
keeping the C file completely separate.
