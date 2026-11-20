# Phase 4.2 — Foreign declaration contract

Phase 4.2 establishes the source-language declaration boundary for C functions.
The parser and AST now represent foreign function declarations, and the type
checker validates their signatures against the initial C ABI subset. Native
foreign-call lowering remains deferred to Phase 4.4.

## Accepted syntax

```fusion
extern "C" {
    fn c_add(a: i32, b: i32) -> i32;
    fn c_message() -> *opaque;
}
```

Each prototype must end in `;`. Parameter types must be explicit. An omitted
return type means `void`. Multiple prototypes may appear in one block.

Only the `C` calling convention is accepted. The initial parser recognizes
`*opaque` as the restricted raw-pointer/handle spelling.

An explicit `#[link_name = "..."]` alias remains planned but is not implemented
in this update. The link symbol currently defaults to the declared function name.

## Initial ABI subset

- `void` return only (not parameters)
- signed/unsigned fixed-width integers: 8, 16, 32, 64 bits
- `f32`, `f64`
- opaque raw pointer/handle (`*opaque`)
- C calling convention only
- no structs by value, unions, variadic calls, callbacks, C++ ABI, strings or
  slices, or automatic ownership transfer

The type checker rejects unsupported ABI types and `void` parameters. Foreign
function names are registered for semantic diagnostics, but calls are explicitly
rejected until native call lowering is implemented.

## Symbol and calling convention rules

- The default linker symbol is the declared function name.
- The initial calling convention is the target platform's C ABI; Fusion does not
  promise a cross-platform binary ABI independent of the target.
- Foreign declarations do not imply that Fusion manages pointee memory. Pointer
  ownership and lifetime remain caller-managed until a later safety/ownership
  design.

## Implementation status

- Lexer: semicolon token for foreign prototypes.
- Parser: `extern "C" { ... }`, required typed parameters, optional return type,
  and restricted `*opaque` spelling.
- AST: foreign ABI and link-name metadata on function declarations.
- Type checker: validates the supported C ABI type matrix and registers foreign
  declarations; rejects calls with a Phase 4.4 diagnostic.
- HIR lowering: retains the declaration name for resolution but does not create
  a Fusion function body.
- LLVM emission: does not emit a fake empty Fusion definition for a foreign
  declaration.

This update does not yet emit C `declare` statements or native calls. Those belong
to Phase 4.4.

## Follow-on milestones

Phase 4.3 defines and validates the ABI-safe type subset; see
`PHASE4_3_ABI_SAFE_TYPES.md`. Phase 4.4 will implement exact LLVM ABI type
mapping and native call lowering, including link symbol handling, then add a
Fusion-source-to-C acceptance test that prints `42`.
