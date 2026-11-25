# Stage 2 — Generic functions and standard-library builtins as values

This patch builds on the Stage 1 interpreter closure implementation.

## Implemented

- Generic user functions may be referenced as values. Their `Type::Generic` variables remain in the callable signature and are inferred from arguments when the value is called.
- Generic function values can be checked against concrete function types, such as assigning `identity<T>` to `fn(num) -> num` or passing it to a function expecting that signature.
- Standard-library builtins registered by `stdlib_ext::signatures()` can be evaluated as first-class `Value::Builtin` values, stored in variables, passed as callbacks, and invoked through the normal function-value call path.
- Builtin values can be captured by Stage 1 closures.
- Function values remain non-serializable/non-copyable through the data-format conversion path.

## Deliberate boundary

Only builtins implemented by `stdlib_ext::is_builtin` are first-class in this stage. Core interpreter special forms and direct-call-only operations such as `print`, `input`, and `open` are not exposed as builtin values yet. They need explicit callable descriptors because their behavior is not uniformly implemented by `stdlib_ext::call`.

Generic function values use inferred specialization at the call site. This patch does not add a new explicit specialization expression syntax (for example, a standalone `identity<num>` expression). Existing explicit generic call syntax remains unchanged.

## Runtime model

`Value::Function(symbol)` continues to represent Fusion functions and Stage 1 closures. `Value::Builtin(name)` represents a standard-library builtin by its stable builtin name. `call_function_value` dispatches the latter through `stdlib_ext::call`; no host function pointers are stored in values.

## Regression coverage added

- Generic identity function stored in a variable and called with inferred type arguments.
- Generic identity function assigned to a concrete function type.
- Generic function passed to a higher-order function expecting a concrete callable signature.
- Standard-library builtin stored in a variable, directly invoked as a value, passed as a callback, and used as a higher-order builtin value (`array_map`).
- Standard-library builtin captured by a Stage 1 closure.

## Validation

The patch was source-reviewed and packaged, but this environment does not have `cargo` or `rustc`. Run `cargo test --test function_values` and then `cargo test` on the project after applying it.
