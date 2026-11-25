# Phase 4.5.2 — Opaque pointer contract foundation

This change introduces a metadata model for opaque pointers at the C ABI
boundary. It deliberately does not change the machine-level C ABI type: an
opaque pointer still lowers to LLVM `ptr`.

## Contract dimensions

- `ForeignPointerOwnership::Borrowed`: the receiver may use the pointer but
  must not release it.
- `ForeignPointerOwnership::Owned`: the caller owns a returned pointer and
  must release it using the declared destructor symbol.
- `ForeignPointerOwnership::Consumed`: a parameter transfers ownership to the
  foreign callee; Fusion must not continue to treat it as owned afterwards.
- `ForeignPointerOwnership::Unspecified`: legacy declaration metadata has no
  ownership guarantee. It must not be treated as equivalent to `Borrowed`.
- `nullable` is independent of ownership.

An owned pointer return is invalid without a non-empty destructor symbol.
Parameter contracts cannot declare a destructor, and `Owned` is not accepted
for a parameter because the direction of transfer would be ambiguous; use
`Consumed` for transfer from Fusion to the callee.

## Current boundary / remaining work

The existing Fusion source syntax does not yet express these contracts. The
current type checker therefore records pointer contracts as `Unspecified` for
legacy `*opaque` declarations. The Rust metadata API can construct and validate
explicit contracts, and the C ABI representation remains unchanged.

This is the contract-model foundation, not yet runtime enforcement. In
particular, this patch does not claim to prevent double release or use-after-
release in generated native code. Those require language-level contract syntax,
propagation into backend lowering, and a runtime/IR handle-state strategy. The
interpreter currently rejects foreign calls, so interpreter-side handle
tracking must wait until interpreter FFI exists.
