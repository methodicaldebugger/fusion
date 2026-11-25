Fusion Stage 1 — Interpreter closures with by-value captures

BASELINE
--------
Prepared against the uploaded forchatgpt(5).zip archive. The Phase 4.5.2
foreign pointer-contract implementation and docs are retained from that
baseline; foreign.rs and docs/phase-4.5.2-pointer-contracts.md are not replaced
by this patch. type_checker.rs includes the baseline Phase 4.5.2 changes plus
the small callable-type changes required by this stage.

APPLY
-----
Extract this ZIP directly into your existing "Fusion core" project directory,
allowing the included files to overwrite their matching files. The ZIP paths
are relative to that directory.

WHAT CHANGED
------------
- AST now preserves an explicit Closure expression with an ordered capture list.
- Parser still hoists lambda bodies to private __lambda_N functions, but adds
  hidden capture parameters and local aliases for the captured names.
- Name resolution validates captured names in the lexical scope where the
  lambda is created.
- Type checking hides the synthetic capture parameters from the public
  function type of the closure expression. Calls through dynamically typed
  captured callable locals are checked at runtime in this first stage.
- Interpreter snapshots captured Values when a closure is evaluated and
  appends those values when invoking the closure. Returned/nested closures work
  through the interpreter-owned closure table.
- Module/HIR traversal recognizes the new AST expression. Legacy AST-based LLVM
  emission rejects closure nodes; project HIR currently retains the hoisted code
  symbol as a compatibility placeholder and is not native-closure-ready.
- Tests cover snapshot capture, callback use, local function-value capture,
  escaping closures, nested closures, and the project pipeline.

LIMITATIONS
-----------
- Interpreter support only; native LLVM closure environments and general
  indirect calls are still future work.
- Captures are by-value snapshots. Mutation is local to the closure invocation
  and is not shared back to the outer binding.
- The interpreter closure table retains capture vectors until the Interpreter
  is dropped. Per-value reclamation awaits an owning closure runtime value.
- Captured resource Values preserve their existing Value::clone semantics;
  Rc-backed resources remain shared handles.
- Captured callable locals whose hidden type is not statically known use
  runtime arity/type validation in this stage.

TESTING
-------
The patch preparation environment does not have cargo/rustc installed, so no
Rust compilation or test run is claimed here. After applying, run from the
Fusion core directory:

    cargo test --test function_values
    cargo test

If either command fails, send the first complete compiler/test error and the
exact applied source archive so the patch can be corrected against that state.
