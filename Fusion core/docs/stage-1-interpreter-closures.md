# Stage 1 — Interpreter closures with by-value captures

## Scope

This stage adds lexical capture for anonymous function expressions in the interpreter. A lambda body remains hoisted into a private `__lambda_N` function, but its expression is now an explicit AST closure node containing the code symbol and ordered capture names.

## Lowering strategy

The parser appends synthetic `__fusion_capture_N` parameters to the hoisted function and emits local aliases from the original lexical names to those hidden parameters. The closure expression records the ordered capture names. This keeps the existing resolver and function-body representation usable without adding a separate statement-body representation for closures.

The type checker removes the trailing synthetic capture parameters when inferring the public function type of a closure expression. At runtime, evaluating a closure snapshots each capture by cloning the current `Value`; named function values are captured as ordinary function values. Calling the closure appends the snapshot values to the hidden parameters.

## Semantics

- Captures are snapshots at closure-creation time; later reassignment of the outer binding does not change the captured value.
- The captured binding is local to the closure invocation. Mutating that local alias does not mutate the outer binding.
- Returned closures can outlive the stack scope in which they were created because the captured values are stored with the interpreter's closure entry.
- Cloning follows the existing `Value::clone` semantics. Container values are cloned recursively; resource values backed by `Rc<RefCell<...>>` retain their existing shared-handle behavior.
- Captured callable locals with no statically known type use runtime call validation in this stage.
- This stage does not implement mutable shared captures, lexical cells, native closure environments, or LLVM indirect-call ABI support.

## Runtime representation caveat

The current implementation stores capture vectors in an interpreter-owned map keyed by unique closure symbols. Entries remain for the lifetime of that interpreter; reclamation when the last function value is dropped is deferred until function values have an owning runtime representation suitable for the language's memory-management model.

## Tests added

- Capture a local value and verify later reassignment does not affect the closure.
- Return a closure from a function and invoke it after the creating scope exits.
- Create multiple closures from separate invocations and verify their snapshots remain independent.
- Run a captured closure through the project compilation pipeline.

## Next stages

Generic function values and built-ins as values remain separate work. Native indirect calls and a code-pointer-plus-environment representation remain Stage 3 work; the HIR compatibility lowering currently represents the closure expression as its hoisted code symbol and therefore is not a native closure implementation.
