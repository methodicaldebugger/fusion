
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

## HIR structural type analysis

The project-aware HIR call resolver carries structural types for primitives,
nominal declarations, tuples, arrays, and supported generic containers. Its
expression inference checks all known array elements, resolves indexed element
and tuple result types, follows struct-field types, and propagates conversion,
`await`, function-call, and method-call return types (including known generic
substitutions). Unknown expressions remain explicitly unknown; this analysis
is an incremental semantic layer and does not yet replace the compatibility-AST
type checker or LLVM body emitter.

## Commands

```text
fusion run program.fusion
fusion check program.fusion
fusion emit-llvm program.fusion -o program.ll
fusion emit-package-llvm program.fusion --package-source-root path/to/package/src -o package.ll
fusion package-interface program.fusion --package-source-root path/to/package/src -o package.fpi
fusion package-declarations package.fpi -o imports.ll
fusion build program.fusion -o program
```

`emit-package-llvm` emits package-owned function definitions and scalar constant
globals. Cross-package function and constant references become external declarations;
Foundry validates those declarations against typed package interface v5 records before
linking. Public struct layouts use stable module-qualified LLVM identified-type names,
and Foundry checks the concrete ordered field layout across package modules before
native linking. Shared runtime state remains owned by the application entrypoint.
Array/nested aggregate field storage, aggregate-by-value function signatures, generic
and method ABI expansion, and non-literal package constants remain outside the current
supported cross-package ABI.

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
## Pattern and tuple language features

The parser/interpreter now support tuple expressions and tuple types such as `(num, string)`, nested tuple patterns such as `(left, (middle, right))`, integer range patterns (`3..6` is end-exclusive), or-patterns (`1 | 2`), and boolean match guards (`value if value > 0`). Struct declarations can also carry Rust-style derive metadata, for example `#[derive(Debug, PartialEq, Clone)] struct Point: ...`.

Tuple expressions are interpreted and type-checked, with native tuple construction/indexing support documented below. Derive attributes are retained on the struct AST and expanded into trait implementations before name resolution and type checking.

Run the regression suite with `cargo test` in a Rust toolchain environment.


## Native tuples and derives

Native tuple values use a tracked heap allocation with a small header. Tuple allocations are linked into a runtime list and released by the generated `main` epilogue. Tuple indexing is supported for non-negative integer literal indices, including nested tuple expressions when their layout is statically known; this restriction is intentional because tuple elements can have different types. Out-of-bounds and non-literal tuple indices are rejected by the type checker.

`#[derive(Clone, Debug, PartialEq)]` expands to ordinary `impl Clone for Type`, `impl Debug for Type`, and `impl PartialEq for Type` AST nodes. The generated methods use ordinary receiver/argument validation and method dispatch. Debug formatting and equality currently delegate to internal runtime helpers.

A leading `#` remains a line comment unless it begins the reserved attribute form `#[`. The lexer emits a `Hash` token for `#[` and consumes other `#...` lines as comments, so `#[derive(...)]` is not swallowed as a comment.

### Derive expansion and trait implementations

`#[derive(Debug, Clone, PartialEq)]` is expanded by the parser pipeline into
ordinary `Trait` declarations (when the built-in trait is not already declared)
and ordinary `impl Trait for Type` AST nodes. These generated implementations
are type-checked and registered through the same implementation pass as
handwritten trait implementations; there is no derive-specific method-signature
injection. The original `derives` list remains on the struct node for tooling and
diagnostics. `Debug` and `PartialEq` currently call small interpreter intrinsics
for formatting and structural equality from their generated method bodies; the
trait implementations themselves are regular AST implementations.


## Multi-file modules (Phase 3)

The CLI resolves `import` declarations relative to the importing source file and
then the entry file's directory. A dotted path such as `import math.vector`
searches for `math/vector.fusion` and `math/vector/mod.fusion`. Imported modules
are loaded recursively before the importing file, and repeated imports are
loaded only once. Missing files and dependency cycles produce explicit errors.

For this bootstrap implementation, imported declarations are still merged into
one compatibility compilation unit, but module identity and statement ranges are
preserved in project metadata. Public/private declaration tables are collected,
and visibility validation follows direct import edges. Cross-module references to
private declarations are rejected. References to declarations in unimported
modules are rejected when those modules are part of the loaded project.


### Module visibility and project metadata

Foundry passes resolved direct dependency source roots to the compiler with `--package-root alias=src-path`. A dependency can then be imported by its manifest alias (for example, `import math` or `import math.geometry`; hyphens in manifest aliases become underscores in source imports); dependency-local imports resolve relative to that dependency source root, and the compiler keeps each dependency source tree isolated. Only public declarations are accessible across package boundaries.

Multi-file loading preserves a project-level module graph via `module_system::load_project_with_metadata`; module identity, statement ranges, imports, and visibility tables are also carried on `ast::Program.modules`. Use `pub fn`, `pub struct`, `pub enum`, `pub trait`, and `pub const` to mark top-level declarations as exported. Imported source modules do not need a `main` declaration; only the entry file does. `pub import module` records a re-export edge.

The current compatibility resolver supports function-level module namespaces:

```fusion
import math as m
from geometry import area

main:
    print(m.double(21))
    print(area(5))
```

`import math` also makes the module qualifier `math` available. Qualified function calls are lowered to stable internal symbols based on the defining module, so two modules may define the same local function name. An unqualified function imported from multiple modules is rejected as ambiguous and must be qualified. Selective imports currently support public functions; selective type/constructor imports are intentionally rejected until those symbol kinds receive module-aware identities. Public function re-exports can be called through a facade module. The HIR module API now also exposes `resolve_qualified_definition` and `resolve_unqualified_definition`: these resolve source names to the original declaration owner's stable `HirDefinitionId`, follow public re-exports, enforce visibility, prefer local declarations, and report ambiguous selective imports. They are not yet wired into the legacy type checker or execution backends.

Qualified user-defined types are accepted in annotations (for example `g.Point` after `import geometry as g`). The parser also accepts qualified struct construction (`g.Point { x: 1 }`), enum construction (`g.Color::Red`) and enum variant patterns (`g.Color::Red`). During project compilation, HIR validation first resolves these source spellings in their module context; a later compatibility lowering rewrites nominal type declarations and references to injective module-qualified internal names. As a result, separate modules can define and use their own `Point` or `Error` types without colliding in the legacy type checker. The source-level module graph remains available from `compile_project_with_metadata`; interpreter and LLVM value/type dispatch have not yet migrated to direct `HirDefinitionId` consumption.


The Phase 3 HIR module API also exposes `hir_type_registry(&Project)`, which preserves struct and enum declaration metadata under module-qualified `HirDefinitionId` keys. It is currently a semantic metadata layer; the legacy type checker and execution backends have not yet migrated to consume it. See `PHASE_STATUS.md` for the migration boundary and remaining work.

Nominal type names can also be resolved through `resolve_hir_type_name(&Project, importing_module, source_name)`. It returns the owning `HirDefinitionId` for local, selectively imported, or module-alias-qualified struct/enum names, and `None` for spellings outside the module declaration namespace (such as built-in/container types). This resolver is part of the staged HIR migration; it does not yet replace legacy type-checker lookup.


The same module-aware layer now exposes `resolve_hir_struct_field` and `resolve_hir_enum_variant`. These APIs first resolve the owning type identity (including import visibility), then retrieve field or variant metadata from the HIR registry. Enum payload type spellings retain declaration order. This gives constructor, field-access, and pattern-checking migrations stable metadata to consume without depending on flattened type names.

Project compilation also performs a module-aware HIR type-reference validation pass over struct fields and enum payloads. It resolves imported custom types in each declaration's owning-module context (including alias-qualified and nested container types) before invoking the compatibility type checker. Expression-level constructors, field access, and enum patterns are still being migrated to stable HIR identities.

The module system also exposes `resolve_hir_type_expressions(&Project)`, which resolves explicit struct and enum constructor expressions to stable HIR declaration identities and validates constructor field sets and enum payload arity. Project compilation runs this structural pass before the legacy flattened type checker. Its result is currently a semantic side table keyed by module and source span; full expression type propagation and backend consumption remain in progress.

The project semantic layer also exposes `resolve_hir_pattern_and_field_expressions(&Project)`. It validates field access for constructor-typed local receivers and resolves typed struct/enum match patterns to their owning HIR declaration IDs. Enum payload binding counts and struct pattern field selections are checked. This remains a conservative inference pass: parameter/call return types and nested member chains are not inferred yet, and the compatibility interpreter/backend have not migrated to consuming the resulting side tables.

The module-aware HIR semantic layer also exposes `propagate_hir_expression_types(&Project)`, which conservatively propagates nominal `HirDefinitionId` values through typed parameters, explicit local annotations, constructor initializers, function calls with declared nominal return types, assignments, and nested struct field access. Its result is a source-span-keyed side table; the legacy type checker and execution backends remain the compatibility authority until their migration is complete.


The module-aware HIR semantic layer also propagates nominal types into match-arm bindings. Enum payload bindings and struct-pattern field bindings retain the original declaration identity, including through nested patterns, and those identities seed expression type propagation inside the arm. See `PHASE_STATUS.md` for current limitations.

The Phase 3 semantic layer now also exposes `validate_hir_nominal_value_types(&Project)`. It checks proven nominal identities at struct-field initializers, enum payload arguments, explicitly typed nominal locals/constants, nominal assignments, and function return boundaries. It intentionally diagnoses only cases where both the expected and actual types resolve to module-qualified HIR identities; primitive, generic, and otherwise-unproven expression types remain with the legacy checker until the HIR type representation is expanded.


The executable module pipeline supports duplicate struct/enum names across modules. Type declarations and references are lowered only in the compatibility AST after HIR validation, leaving `Project.modules` and HIR metadata in source-level form. The interpreter now uses HIR identities for nominal definitions, fields, variants and method dispatch; trait and constant declarations also receive module-qualified symbols. LLVM continues to use compatibility symbols for its current type representation while emitting HIR identity manifests.

#### Module-qualified nominal identities in execution and LLVM output

The interpreter now retains a mapping from lowered nominal type symbols to their stable module-qualified HIR declaration identities. It can recover the identity for runtime struct and enum values without conflating same-named declarations from different modules. LLVM output also includes a deterministic `fusion.type` comment manifest mapping backend symbols back to source module/type identities. This is an incremental bridge: runtime values still carry collision-free compatibility symbols, and LLVM's current type representation has not yet been replaced with HIR-ID-native layouts.
Runtime nominal compatibility checks use the recovered HIR identities when available, with exact internal-symbol comparison retained as a fallback for standalone programs that have no project module metadata.

#### Phase 3 module identity continuation

Fusion's module compiler now retains HIR identities for impl methods and enum variants in addition to nominal struct/enum declarations. The interpreter prefers receiver-identity-based method dispatch, and LLVM output carries deterministic method identity metadata. Trait implementations and nominal method calls are resolved against module/import scope and checked for required method signatures. Struct fields and enum variants carry owner-scoped HIR identities for interpreter dispatch. Traits and constants are lowered to injective module-qualified compatibility symbols, including qualified and selectively imported constant references with local-shadow protection. The compatibility AST is still used by the legacy checker/backends; see `PHASE_STATUS.md` for the remaining HIR-native migration work.

The project type checker now has an HIR-aware entry point (`TypeChecker::check_with_hir`). Project compilation validates each lowered nominal symbol against the `HirTypeRegistry`, stores its `HirDefinitionId` in the type environment, and compares nominal struct/enum types by declaration identity rather than by compatibility symbol spelling. LLVM also retains project struct and enum payload layouts in HIR-ID-keyed tables, while preserving the old string-keyed struct table only for standalone compilation. Project body checking now prefers HIR-ID-keyed struct and enum definition tables; early declaration registration and some trait/method tables remain compatibility-symbol based. LLVM retains HIR-ID-keyed struct and enum payload layout tables, but instruction emission still consumes the compatibility AST. The remaining HIR-native layout migration is tracked in `PHASE_STATUS.md`.

### Module-aware compiler internals (Phase 3)

Project method and trait lookup now prefers stable HIR declaration identities instead of compatibility-AST spellings. The LLVM backend emits deterministic named struct, enum, and enum-variant payload layouts from those identities. Struct construction, field reads/writes, and enum construction now use HIR-keyed field/variant identities to emit typed GEP/load/store operations. Nominal values use a program-lifetime tracked heap arena; enum pattern/match CFG lowering, ownership analysis, and the remaining compatibility-AST symbol paths are still in progress.

LLVM project lowering now supports nominal `match` statements through stable HIR identities. Enum arms dispatch on declaration-order discriminants and extract payload fields from variant-specific layouts; nested enum payload patterns recursively compare nested discriminants. Struct patterns resolve each selected field through its owner identity and HIR field index, both at the top level and inside enum payloads. Identifier and `@` bindings are scoped to their match arm, and same-spelled variants/types from another module are rejected by identity. LLVM match lowering also supports scalar literal patterns, tuple destructuring, integer ranges, and short-circuit or-patterns. The type checker tracks boolean and enum coverage, rejects overlapping integer ranges and unreachable arms, and does not count guarded arms toward exhaustiveness. CFG return analysis now recognizes fully returning `if` and exhaustive `match` branches, avoiding invalid fall-through blocks. General integer/float/string exhaustiveness still requires a wildcard; see `PHASE_STATUS.md` for current backend coverage.

LLVM control-flow lowering now supports `break` and `continue` in `while`, counted `for`, and array `for-each` loops. A nested loop-target stack ensures each statement targets the innermost loop; `continue` in counted/for-each loops passes through the increment step, while `break` exits directly. The backend diagnoses either statement outside a loop. Cleanup/defer-aware loop exits and labelled loops remain future work.

The compiler now exposes `lower_project_hir(path)`, which returns a `HirProject` containing separate `HirModule` nodes and module import metadata. HIR lowering assigns stable IDs to top-level functions, structs, enums, traits, constants, fields, variants, and inherent/trait methods before compatibility symbol mangling. Function nodes preserve generic parameters, async flags, parameter/return annotations and bodies. Match guards and recursive pattern forms, conversions, await expressions, method-call generic arguments, and `for-each` loops are represented in HIR. Project compilation runs this lowering stage before the existing HIR semantic validation and compatibility type-checking pipeline; call-site resolution and HIR-native backend execution are still in progress.


## Phase 3 continuation: HIR-resolved calls

The project HIR now carries a `resolved_calls` table. Each known top-level function or nominal method call is associated with its module-qualified `HirDefinitionId`, call span, receiver identity (for methods), nominal argument/parameter identities, return identity, generic parameter list and async status. The resolver checks argument count, explicit generic arity and known nominal argument compatibility.

For project-aware native emission, use `LLVMBackend::emit_project(&project)` rather than the standalone `emit_program` entry point. The project-aware path lowers HIR, registers top-level functions and impl methods by declaration identity, and emits function/method calls through the registered declaration-to-backend-symbol mapping. Native instance methods pass the receiver as LLVM argument zero; an explicit first `self` parameter is supported, and methods without one receive a hidden receiver ABI argument. Trait implementation selection is statically resolved by HIR; dynamic trait objects/vtables are not yet supported. Project LLVM emission now routes resolved top-level and method calls through HIR declaration identities at each module-qualified call site (module plus source span), with compatibility-name lookup retained only as a fallback for standalone/legacy paths. Trait conformance is validated during HIR lowering: optional explicit `self` is normalized out of user-argument matching, annotated receivers must match the implemented nominal type, and generic/async impl methods are rejected until trait signatures can express those contracts. Primitive and generic call checking still relies partly on the existing checker until the compiler adopts a unified resolved type representation.

### Structural HIR call signatures (Phase 3)

Project HIR call records now preserve structural types for primitive values, nominal declarations, arrays, tuples, standard generic containers, and generic nominal applications. Known call arguments are checked against those shapes; generic parameters can be inferred from arguments or seeded by explicit generic type arguments, and inferred bindings are substituted into the resolved return type. Trait implementation conformance compares structural signatures across module boundaries, while trait calls continue to use static HIR-selected dispatch. Unknown expression types are intentionally left conservative. Generic nominal constraints/arity, complete expression inference, associated types, dynamic trait objects, and direct HIR-native body emission remain future work; the compatibility AST remains part of the current backend pipeline.


### Structural HIR local types (Phase 3)

The project HIR now carries `resolved_local_types`, a source-span-keyed record of
structural types inferred for parameters and local bindings. Primitive and generic
types are retained through array/tuple construction, indexing, local assignments,
and generic function calls. Known incompatible local assignments and return values
produce semantic diagnostics. This is an intermediate migration layer: native LLVM
body emission does not yet consume these local records directly.


### Backend-oriented HIR bodies (Phase 3)

`HirProject` now also exposes `bodies`: a parallel backend-facing representation for
functions, implementation methods, and module entry bodies. Each `HirBody` has
module/declaration identity, stable local IDs, expression type annotations, local
reference paths, and explicit basic blocks/terminators for branches, loops, matches,
returns, breaks, and continues. This is the first HIR control-flow form intended for
native backend consumption. It currently coexists with the source-preserving HIR;
LLVM body emission has not yet switched over. Calls and member expressions whose
resolved expression type cannot be proven remain `Unknown`, and source spans for
individual identifier uses are not yet retained in `HirExpression`.

### Incremental direct HIR-to-LLVM emission

The LLVM backend now has a direct `HirBody` emitter for an initial scalar subset and attempts it for eligible top-level functions before falling back to AST emission when a feature is not yet supported. The direct subset covers scalar local IDs, integer/boolean expressions, basic arithmetic/comparisons, assignments, returns, and explicit branches. Loops, matches, calls, nominal/container values, and other unsupported constructs still use the compatibility AST backend.

#### Direct HIR control-flow emission (Phase 3 continuation)

The incremental direct HIR-to-LLVM path now emits counted integer `for` loops and scalar match dispatch for wildcard, integer, boolean, integer-range, and `or` patterns. Match arms are tested in source order and can include boolean guards. The direct path still rejects foreach iteration, destructuring or nominal pattern binding, and non-scalar match values; project compilation retains the AST backend as a compatibility fallback for those cases.

#### Direct HIR function and method calls (Phase 3 continuation)

The backend-oriented HIR body now carries resolved call references by expression-tree
path. For supported signatures, LLVM emits function and method calls directly from
`HirBody`, evaluating arguments in order and selecting the callee by its
`HirDefinitionId` rather than source spelling. Method calls pass the receiver as
argument zero. The current direct subset accepts known scalar values and nominal
values represented as opaque pointers, with a non-void result. Unsupported or
ABI-mismatched calls continue through the compatibility AST emitter. Generic/async
calls, closures, unknown-signature built-ins and void-valued calls are not yet fully
HIR-native; see `PHASE_STATUS.md` for the exact boundary and verification status.


#### Direct HIR foreach and scalar pattern bindings (Phase 3 continuation)

The direct `HirBody` LLVM emitter now supports array `for each` loops using Fusion's existing tracked array storage. It evaluates the collection once, reads its length, loops over the encoded element slots, decodes supported scalar/pointer element types, and assigns each value to its HIR-local slot. Homogeneous array/tuple literal allocation is also supported for integer, boolean, float, and pointer-like values.

HIR match targets now preserve pattern-binding local IDs and scrutinee-relative paths. Direct scalar matches can bind an identifier or an `@` pattern before evaluating a guard, allowing guard expressions to use the bound local. Tuple destructuring and nominal struct/enum payload bindings are still handled by the compatibility AST backend; unsupported direct constructs fall back rather than being silently accepted. Rust regression tests were added but could not be run in the current environment because the Rust toolchain is unavailable.


The package aggregate regression now includes a native-link execution stage when Clang is available: it links the independently emitted provider, consumer, and application LLVM modules, then verifies cross-package struct field access and both integer- and string-payload enum variants at runtime. Run the Rust test suite to execute this integration check; it requires Clang in `PATH`.

### Phase 3 real-toolchain integration smoke test

On a host with Cargo/rustc, Clang, Bash, and Python 3.11+, run:

```sh
bash scripts/phase3-real-toolchain-smoke.sh
```

The script builds the compiler and Foundry, runs the focused cross-package aggregate and Foundry orchestration tests, builds and executes the three-package fixture through the real Foundry CLI, and verifies the generated provenance and package-cache hashes. Set `PHASE3_KEEP_WORKSPACE=1` to retain the temporary fixture workspace for inspection. The expected executable output is `7`, `9`, `failed`.


## Type expressions

Written types are parsed into the structured `type_expr::TypeExpr` (named types with generic arguments,
`T[]`, tuples, `fn(A, B) -> R` function types and `*opaque`). The AST still stores annotations as strings, but
that string is always the canonical `Display` form of a `TypeExpr`, and consumers re-read it with
`TypeExpr::parse` instead of prefix tests such as `starts_with("Option<")`. The type checker converts types
through one function (`convert_type_expr`) for both plain and generic-scoped contexts. Generic arguments and
`[]` now compose in any order (`Option<num>[]`, `num[][]`, `HashMap<string, num>[]`). Function types are
parsed and printed but rejected by the type checker until function values exist.

Not yet migrated to `TypeExpr` (they still split the canonical spelling by hand and handle every spelling the
parser currently produces except function types): the interpreter's `type_from_name` / `value_matches_type`,
`module_system.rs` (`validate_hir_type_spelling`, `split_hir_type_arguments*`, the HIR type-spelling parsers),
`hir_body.rs::lookup_type` and the LLVM backend's `type_from_name` / `llvm_type_name`.

## Trait default methods

A trait method written with a body is a default method. `trait_defaults::expand_trait_defaults` runs right after
`#[derive]` expansion and copies each default into every `impl Trait for Type` that does not define it, replacing
`Self` in the signature with `Type`. The rest of the compiler only sees ordinary impl methods. Limits: the trait
and the impl must be in the same source file, default methods cannot be generic, and trait objects, associated
types, supertraits and generic traits are still unimplemented.

## Recursion limits

The interpreter is a recursive tree walker, so Fusion recursion consumes host stack. `fusion run`, `fusion test`
and `fusion bench` therefore execute on a thread with a 512 MiB reserved stack
(`interpreter::run_with_large_stack`), and the interpreter stops with
`Maximum call depth (10000) exceeded while calling '<name>'` instead of overflowing the host stack and aborting
the process. Task threads (64 MiB stacks) use a proportionally lower limit. The limit is
`interpreter::MAX_CALL_DEPTH` and can be changed per interpreter with `Interpreter::set_max_call_depth`.

## Generic structs and enums

```
struct Pair<A, B>:
    first: A
    second: B

enum Maybe<T>:
    Just(T)
    Nothing

struct Stack<T>:
    items: T[]

impl Stack<T>:
    fn size(self: Stack<T>) -> num:
        return self.items.get_length()
```

Type parameters follow the struct, enum or impl type name (`struct Name<T, U>`, `enum Name<T>`,
`impl Name<T>` and `impl Trait for Name<T>`), in both indentation and brace style.

**Generics are erased, not checked.** The parser spells each parameter `unknown` inside its declaration
(`first: unknown`) and drops type arguments on user types wherever they are written (`Pair<num, string>`
becomes `Pair`), so the type checker, HIR passes and interpreter only ever see ordinary types. Built-in
constructors (`Option`, `Result`, `HashMap`, `Array`, `Iterator`, `Task`) keep their arguments. Consequences:

- `Pair<num, string>` is documentation only: `Pair(first: "x", second: 1)` is accepted for it.
- Values read from an erased field have type `unknown`, which the checker accepts everywhere.
- Explicit constructor arguments (`Stack<num>(items: [])`) are not supported; write `Stack(items: [])`.
- Generic traits (`trait Name<T>`) and generic default methods are not supported.
- The LLVM backend does not lower `unknown`-typed fields, so generic types are interpreter-only for now.

Typed generics (checking arguments and monomorphizing for the native backend) are future work and would
make some programs accepted today an error.
