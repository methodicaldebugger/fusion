# Fusion roadmap status — audited against this repository snapshot

## Phase 1 — Language fundamentals

**COMPLETED for the current project milestone (confirmed by the project owner).**

Implemented language foundations include:
- Tuple types and runtime tuple support.
- Nested enum, tuple and struct patterns; `@` bindings; match guards; range and or-pattern support.
- Fixed-width integer/float type names and interpreter-side range/cast checks.
- Built-in struct derives (`Debug`, `Clone`, `PartialEq`) and initial operator dispatch (`Add`/`Index`/`Deref`-style methods).

Remaining backend-parity and language-hardening work is tracked separately; it does not block the Phase 1 milestone:
- LLVM exact-width signed/unsigned integer arithmetic and `f32` emission.
- Native backend parity for custom operator dispatch and all pattern forms.
- A general attribute/procedural derive system beyond built-in struct derives.

## Phase 2 — Native runtime

**COMPLETED for the current project milestone (confirmed by the project owner).**

The native runtime milestone includes tracked heap allocation for native array literals, array element type tracking and bounds checks, tuple/array length support, and interpreter wide-integer representation and arithmetic.

Remaining native-runtime parity and hardening work is tracked as follow-up:
- Exact-width integer and `f32` LLVM emission.
- Native `break`/`continue`, full `match`, and cleanup-aware `defer` lowering.
- Runtime string/collection APIs with ownership semantics.
- Closure/captured-environment representation.
- A defined native memory reclamation/GC strategy.

## Phase 3 — Modules and packaging

**Core source-module loading and function-level namespace lowering are implemented; package dependency integration and complete type-level namespaces remain incomplete.**

Implemented:
- Recursive dotted-import loading from sibling/nested `.fusion` and `mod.fusion` files.
- Dependency-first loading, missing-file diagnostics and import-cycle detection.
- Per-file syntax-style detection; imported modules do not require `main`.
- Only the project entry file may declare `main`.
- `Project`/`ModuleUnit` metadata and source statement ranges are retained after lowering.
- `pub` declarations, private symbol metadata, direct-import privacy checks and re-export propagation.
- Module aliases (`import math as m`) and qualified function calls (`m.double(...)`).
- Unaliased module-qualified function calls (`math.double(...)`).
- Selective function imports (`from math import double`). The initial selective-import resolver deliberately rejects non-function imports until type/constructor namespaces are lowered too.
- Qualified user-defined type syntax (`geometry.Point`) is accepted in type positions. The type checker exposes imported public struct/enum definitions through module aliases, so qualified parameter/return/field type references can resolve when the legacy flattened declaration name is unambiguous.
- Stable, injective internal function symbol names derived from module identity and local function name.
- Same-named functions, nominal structs/enums, traits and constants in different modules are lowered to collision-free internal symbols in the compatibility AST. The latest continuation adds HIR identity maps and runtime dispatch for nominal members.
- Unqualified imported function calls work when unambiguous; ambiguous imports and colliding default module aliases produce diagnostics requiring qualification or explicit aliases.
- Public function re-exports can be called through a facade module.
- Privacy/reference checks now walk AST nodes rather than searching debug-formatted AST text.
- CLI `run`, `check`, `emit-llvm` and `build` compile from the entry file and recursively load imports. Foundry's `check` and `build` use the project's entry file rather than compiling every source independently.
- Regression tests cover aliases, qualified calls, function namespace isolation, ambiguous imports, selective imports, re-export calls, nested imports, private access and entry/module `main` rules.

New work in this continuation:
- Added `HirDefinitionId` with module, nested scope, local name and declaration kind. This gives structs/enums/traits/constants/functions/methods a collision-free semantic identity independent of backend mangling.
- Added `HirSymbolIndex` and a `hir_symbol_index(Project)` builder that indexes declarations per module and scopes trait/inherent implementation methods by their owning type.
- Added regression tests for same-named type identities across modules, method identities across impl targets, and module-index population.
- Preserved original top-level function spellings separately from compatibility-AST mangling. `hir_symbol_index(Project)` now indexes functions as `(module, source name, kind)` rather than leaking backend-oriented `__fusion_...` names into semantic identities.
- Added a regression test ensuring the HIR symbol index contains `math::double` and does not expose its compatibility-mangled spelling.
- Added `resolve_qualified_definition`, resolving module aliases and public re-export chains to the original declaration ID, with private/missing-symbol diagnostics.
- Added `resolve_unqualified_definition` for local declarations and selective imports. Local declarations take precedence; selective imports resolve to the original provider; conflicting providers produce an explicit ambiguity diagnostic instead of choosing by traversal order.
- Added regression coverage for selective-import identity resolution, ambiguous selective imports and local declaration precedence.
- Added `HirTypeRegistry` and `hir_type_registry(Project)`, a module-qualified metadata store for struct and enum declarations. It retains field/variant payload metadata and visibility alongside each stable `HirDefinitionId`, so same-spelled declarations can coexist in the HIR layer without entering the legacy flat type tables.
- Added regression coverage for registry construction, struct fields, enum payloads, visibility, and two distinct `Point` declarations in separate modules at the registry level.

Outstanding:
- The HIR identity/index and type-registry layers are not yet the sole compiler authority. `HirTypeRegistry` preserves source declarations and is used by semantic validation, while the interpreter now dispatches fields, variants, and methods through HIR identities before compatibility fallbacks. The LLVM backend still consumes the compatibility AST and emits identity manifests rather than HIR-native layouts/instructions. Qualified type references remain an incremental type-checker bridge.
- The parser now accepts qualified struct construction using `g.Point { x: 1, y: 2 }` and qualified enum construction/patterns using `g.Color::Red`. Regression cases were added for qualified struct construction, enum construction, and enum variant patterns. These paths still depend on the legacy alias tables and do not yet establish stable `HirDefinitionId` references.
- Duplicate same-named structs/enums are now permitted across modules. After source-level HIR validation, the compatibility AST receives module-qualified internal type names for declarations, annotations, constructors, conversions, and patterns. The interpreter uses HIR identities for runtime nominal compatibility, field/variant checks, and method dispatch; LLVM retains corresponding identity metadata but still lowers from compatibility AST names.
- Selective imports now pass through the executable namespace bridges for public functions, nominal types, traits and constants. The HIR resolver provides stable declaration identities for these symbols, but the legacy checker still consumes their lowered compatibility names; per-item aliases and some re-export edge cases remain future work.
- Privacy checks are AST-structural but are not yet based on resolved `DefinitionId`/symbol identities; shadowing and complex re-export cases need a proper semantic import table.
- Qualified module calls currently lower to internal function names before the legacy resolver/type checker. This is a compatibility bridge, not the final HIR/module-aware resolution architecture.
- Native multi-file compilation/linking still needs end-to-end validation with the LLVM backend and Foundry on Windows. The CLI routes project builds through the entry-file compiler API, but the HIR-native backend path is not complete.
- Foundry package dependencies and registry packages do not yet feed the compiler's module graph; source imports are currently resolved within the source tree. Package graph resolution, dependency source roots, and compiler module-graph integration remain outstanding.
- A single stable project build contract, package version conflict handling, workspace support and dependency lockfile integration remain unfinished.


## Verification

The current execution environment does not contain `cargo` or `rustc`, so no Rust compilation, formatting, or tests can be truthfully reported as passing. Run `cargo fmt --check` and `cargo test --workspace` (or the repository's supported package-by-package test commands) on a Rust-enabled machine before merging.

## 0.3 standard library (unverified build)

Added: regex/UTF-8, JSON/TOML, SHA-256/HMAC/OS randomness, dates with fixed offsets, TCP/UDP,
HTTP/1.1 client and request/response server toolkit, file/path/process utilities, share-nothing
tasks/channels/parallel_map, `#[test]`/`#[bench]` runner, `fusion doc`. See `docs/stdlib.md`.
Not done: TLS, HTTP/2, named time zones, work-stealing scheduler, concurrent `async`, Foundry
integration of `fusion test` / `fusion doc`.

### Latest Phase 3 continuation — HIR import resolution

- Added `resolve_qualified_definition`, which resolves an import alias and exported
  symbol to the declaration owner's stable `HirDefinitionId`.
- Qualified lookup follows public re-export chains and rejects private or missing
  declarations. Alias spellings do not leak into semantic identities.
- Added regression tests for aliased type lookup, re-export resolution, and private
  declaration rejection.
- Added both qualified and unqualified HIR lookup primitives. Unqualified lookup observes local declarations first, then selective imports, and reports ambiguity when multiple modules provide the same name.
- These are HIR-facing resolver primitives; the legacy type checker and execution backends do not yet consume them as their authoritative lookup path. Duplicate type names therefore remain guarded until that migration is complete.
- Rust formatting and tests remain unverified in this environment because
  `cargo`, `rustc`, and `rustfmt` are unavailable.

### Latest Phase 3 continuation — module-aware type metadata

- Added a separate `HirTypeRegistry` keyed by complete `HirDefinitionId` values. Struct and enum declarations retain their source module identity, visibility, struct fields/derive names, and enum variants/payload type spellings.
- Added `hir_type_registry(Project)` to construct this metadata from the module graph without changing the legacy type-checker tables.
- This is an intentionally non-breaking migration seam: duplicate type names can coexist in the registry API, but project validation still rejects them before executable compilation until all downstream type consumers are migrated.

### Phase 3 continuation: nominal type-name resolution

- Added `resolve_hir_type_name(Project, importing_module, source_name)`, which maps an unqualified local/selectively imported type or an `import ... as ...` qualified type name to its owning `HirDefinitionId`.
- Resolution follows public re-export chains through the existing HIR declaration resolvers and preserves the original provider identity. Private/missing imported declarations are not silently treated as valid types; ambiguous struct/enum spellings are reported.
- Built-in/container/generic spellings that are not module declarations return `Ok(None)` so a later type checker bridge can handle primitive and parameterized types separately.
- Added tests for import aliases, selective type imports, and built-in type spellings.
- This is still a resolver API, not yet wired into `TypeChecker::convert_type`; compatibility AST type maps remain name-keyed and duplicate-type validation remains enabled until that migration is complete.


### HIR member resolution continuation

- Added `resolve_hir_struct_field(Project, importing_module, source_type_name, field_name)`. It resolves the type through the HIR type-name resolver, enforces imported type visibility, verifies the declaration is a struct, and returns the owning `HirDefinitionId` plus cloned field metadata.
- Added `resolve_hir_enum_variant(Project, importing_module, source_type_name, variant_name)`. It performs the corresponding enum-kind and variant lookup and preserves payload type order.
- Regression coverage includes alias-qualified field lookup, missing fields, enum payload ordering, and rejection of private imported types.
- These APIs are metadata/resolution primitives; AST expression checking, constructor validation, pattern checking, interpreter dispatch, and backend lowering still consume the compatibility representation. The duplicate-type guard remains until those consumers are migrated.

### Latest Phase 3 continuation — project-level HIR type-reference validation
- Added `validate_hir_type_references(Project)`, which walks the HIR type registry and resolves custom types used by struct fields and enum payloads in the declaring module's import context.
- The validator recursively handles the built-in container spellings (`Array`, `Iterator`, `HashMap`, `Option`, `Result`, `Task`), array suffixes, tuples, and primitive/fixed-width built-in types.
- Unknown custom type names and invalid generic arities now produce diagnostics that identify the module, owning type, and field/variant payload position.
- Wired this validation into both `compile_project` and `compile_project_with_metadata`, before the legacy flattened-AST type-checking pass. This means project compilation now performs a real module-aware semantic check over type declarations rather than leaving the HIR registry as passive metadata.
- Added regression coverage for alias-qualified custom types nested inside containers and enum payloads, unknown field types, and private imported types.
- This is declaration-level validation only. It does not yet resolve expression-level struct/enum constructors, field expressions, or match patterns to HIR IDs, and it does not remove the duplicate-type guard.

### Constructor-expression identity bridge (latest continuation)

- Added `resolve_hir_type_expressions(Project)`, a project-wide AST walk that resolves explicit struct and enum constructor expressions in the context of the module that contains them.
- Struct constructors are resolved to their original `HirDefinitionId`; supplied fields are checked for unknown names, duplicates and omissions against the owning declaration.
- Enum constructors are resolved to their owning enum identity and variant; payload arity is checked and the variant's declared payload type spellings are retained in the resolution record.
- The pass recursively visits constructor expressions nested in variable initializers, constants, assignments, function bodies, control flow, match guards/bodies, calls, method calls, arrays, tuples, and other expression trees.
- `compile_project` and `compile_project_with_metadata` now run this pass after declaration type-reference validation and before the legacy flattened compiler pipeline.
- Regression tests cover alias-qualified struct and enum construction, stable owner identities, payload metadata, unknown/duplicate/missing fields, and incorrect enum payload arity.

Limitations deliberately retained:
- The result is a module-and-span keyed semantic side table, not yet an annotation embedded in every HIR expression node.
- Constructor payload/field *value types* are still checked by the existing type checker where its compatibility representation supports them; this new pass validates declaration identity and structural shape, not full expression type compatibility.
- Field access and enum pattern bindings still need expression/pattern type propagation before the duplicate-type guard can be removed.

## Phase 3 — expression member and pattern resolution

- Added `resolve_hir_field_accesses(Project)`. It resolves property access when the receiver is a struct constructor or a local variable initialized from one, and validates the requested field against the owning module's HIR type declaration.
- Added `resolve_hir_pattern_and_field_expressions(Project)`, which returns stable-identity side tables for resolved field accesses and typed enum/struct patterns.
- Enum patterns are checked against the match scrutinee when its type is known from a direct enum constructor or a local initialized from one. Qualified variant paths resolve through module aliases; variant payload binding counts are validated.
- Struct patterns validate their resolved struct identity, field names, and duplicate field selections.
- Both project compilation entry points now run this semantic pass after constructor resolution.
- Regression tests cover constructor-initialized local field access, unknown fields, enum patterns through aliases, and payload binding-count errors.
- Deliberate boundary: this is not general expression type inference. Function parameters, function-call results, returned values, nested property chains, and unqualified nested payload patterns still require type propagation. The pass records only types it can establish without guessing. The legacy interpreter/backend still do not consume these side tables, and duplicate type names remain guarded.

### Latest Phase 3 continuation — nominal expression type propagation

- Added `HirResolvedExpressionType` and `propagate_hir_expression_types(Project)` as a conservative semantic side table keyed by source module, expression span, and stable `HirDefinitionId`.
- Nominal identities now propagate from explicit local type annotations, typed function parameters, struct/enum constructors, local variables, assignments, and function calls with explicit nominal return types.
- Struct property chains propagate through field declarations using the field owner's module as the type-name resolution context. For example, a `Person` parameter can establish `person.address` as `Address`, then `person.address.city` as `City`.
- Function calls are recognized after compatibility-AST name mangling by recovering the source declaration from the module's original function-symbol inventory.
- The pass is invoked by both project compilation APIs after constructor and pattern/field validation. It records only proven nominal types; builtin/generic types and unsupported inference cases are intentionally omitted.
- Added regression cases for imported function return propagation through nested fields, typed parameter propagation, and explicit local nominal annotations.
- This is still a side table, not yet the authoritative type checker. It does not yet infer generic/container element types, match-pattern payload bindings, arbitrary function return inference, or closure/captured variable types. The interpreter and LLVM backend do not yet consume these identities. Duplicate type names therefore remain guarded.


### Latest Phase 3 continuation — nominal types for pattern bindings

- Added `HirResolvedPatternBinding` and `resolve_hir_pattern_binding_types(&Project)`.
- Enum variant payload bindings inherit nominal identities from the selected variant's declared payload types. Payload spellings are resolved in the enum's declaring module, while the recorded binding remains associated with the use-site module and pattern span.
- Nested enum patterns propagate payload identities recursively. Struct pattern field bindings inherit the declared field type from the owning struct. `@` patterns bind the whole matched value as well as recursively processing the nested pattern.
- Or-pattern alternatives are analyzed in isolated environments; only bindings with the same nominal identity in every alternative escape the pattern. Tuple-pattern element types remain unavailable because tuple element types are not represented in `HirTypeShape`.
- Expression type propagation now seeds each match-arm environment from its pattern bindings, so field chains used inside an arm can resolve from the payload type.
- Both project compilation entry points invoke pattern-binding type resolution before expression type propagation. Regression tests cover enum payloads, nested enum patterns, struct field bindings, and nested property access.
- This remains a conservative semantic side table. It does not yet replace the legacy type checker or annotate interpreter/LLVM values, and duplicate type names remain guarded.

### Latest Phase 3 continuation — nominal value-boundary validation

- Added `validate_hir_nominal_value_types(&Project)` and wired it into both project compilation entry points after nominal expression propagation.
- Struct constructor field values are checked against the declared field type using the struct's declaring module for the field type spelling and the constructor's use-site module for the value expression.
- Enum constructor payload values are checked against variant payload declarations in the enum's declaring module.
- Explicit nominal local/constant annotations, assignments to nominal locals, and function return expressions are checked when both expected and actual types resolve to stable module-qualified `HirDefinitionId` values.
- The pass is intentionally conservative: it does not claim primitive/generic compatibility or infer arbitrary expression types. Those remain in the legacy checker until the HIR type model and backend migration are complete.
- Added regression tests for mismatched struct fields, enum payloads, and function returns, plus matching imported nominal values.
- Tests were added but could not be executed in this environment; the source archive is checked for ZIP integrity separately.


### Latest Phase 3 continuation — executable nominal type namespaces
- Project validation now permits same-named struct/enum declarations in different modules while continuing to reject collisions in flat trait/constant namespaces.
- Added `mangle_type` and `lower_project_type_namespaces`. This lowers local and imported (alias-qualified or selective) nominal type references to injective module-qualified internal names in the compatibility AST only.
- The lowering covers struct/enum declarations, field and payload type spellings (including nested generic/container spellings), function/parameter/local/constant annotations, struct and enum constructors, conversion targets, impl type names, and struct/enum patterns.
- HIR validation runs before this compatibility lowering. `Project.modules` retains source spellings and source-level module metadata; the legacy type checker sees collision-free names.
- Added regression tests for two modules defining `Point`, constructing and returning their own `Point` types, using both imported functions together, and rejecting an assignment across the two nominal identities.
- Remaining boundary: this is an execution-pipeline bridge. Runtime values still carry collision-free compatibility symbols and recover their HIR identities through tables; LLVM does not yet use HIR IDs as native type-layout keys.

### Latest Phase 3 continuation — runtime and LLVM nominal identity bridge
- Added `backend_type_definition_ids(Program)`, which reconstructs stable `HirDefinitionId` values from the project module ranges and the injectively mangled compatibility-AST type symbols.
- The interpreter now retains this identity map when registering declarations and exposes lookups for a backend type symbol or a runtime struct/enum `Value`. Runtime values continue to store their collision-free internal symbol, while the interpreter can recover the originating module, source name, and declaration kind.
- The LLVM backend now retains the same identity map and emits a deterministic `; fusion.type ...` manifest in generated IR. This preserves source-level type identity for downstream tooling without changing LLVM's current opaque-pointer representation.
- Added a regression test proving that two modules' `Point` declarations map to distinct stable identities after namespace lowering.
- This is an identity bridge, not yet a full HIR-native backend: runtime values still store compatibility symbols rather than embedding `HirDefinitionId`, and LLVM layout/type lowering still uses the compatibility AST. The next step is to make constructor, pattern, field, and method dispatch consume the identity directly, then migrate traits/constants.
- Runtime nominal compatibility checks now compare the recovered `HirDefinitionId` values when both sides have project metadata, falling back to exact internal-symbol equality only for standalone/legacy values. This makes runtime type acceptance identity-based rather than relying solely on the display spelling.

### Latest Phase 3 continuation — identity-based dispatch and symbol namespaces

- The interpreter now builds an additional method-dispatch table keyed by `(HirDefinitionId, method name)` for the receiver type. Struct method calls and operator overloads consult the receiver's stable HIR identity before using the compatibility-symbol fallback.
- Impl methods receive scoped HIR method identities (`module + inherent/trait scope + receiver type + method name`). `resolve_hir_method` / `resolve_hir_method_by_id` now resolve method calls from receiver identities, prefer inherent implementations, enforce trait visibility through selective or whole-module imports, and report ambiguous/missing providers. Nominal method calls encountered during expression propagation are checked and nominal return types can propagate from the selected implementation. The LLVM backend retains the same method identity table and emits deterministic `fusion.method` manifest entries alongside nominal type identities.
- Enum variant resolution now goes through the owning enum's HIR identity. Struct fields and enum variants also have scoped `HirDefinitionKind::Field` / `Variant` identities in the symbol index and LLVM identity manifest. The interpreter maintains HIR-keyed struct/enum definition tables and validates constructors, field reads/writes, struct patterns and enum patterns against their owner identities.
- Added project-level trait implementation resolution. Trait references are resolved in the implementation's module/import scope, receiver types resolve to nominal HIR identities, and required trait methods are checked for presence and compatible parameter count/return type.
- Trait declarations and constants now receive injective module-qualified compatibility symbols. Qualified module-constant property expressions are rewritten to those symbols; constant identifier rewriting tracks lexical variable/parameter shadowing to avoid rewriting a local variable that happens to share a constant's name.
- Duplicate trait and constant names in different modules are no longer rejected merely because the compatibility AST used flat names. Selective non-function imports are allowed through the function-import pass when their declaration is handled by the type/trait/constant namespace resolver.
- The compatibility AST remains in use by the legacy type checker and code generators. The interpreter now maintains HIR-keyed struct/enum definition tables in parallel with compatibility maps; constructors, field access, patterns and method dispatch prefer those identity-keyed tables. LLVM still uses compatibility symbols for its current layouts, while its manifest preserves type/member/method/trait/constant identities. Constant/trait namespace lowering is still a compatibility bridge, not a fully HIR-native symbol table.
- Regression tests were added for same-named constants/traits in separate modules, selective constant imports with local shadowing, inherent and trait method dispatch, missing trait methods, scoped field/variant IDs, and enum runtime metadata. They have not been executed in this environment because Rust tooling is unavailable.

### Latest Phase 3 continuation — HIR-authoritative nominal type checking and layout indexing
- Added `TypeChecker::check_with_hir`, the project compilation entry point for nominal checking. It requires every lowered struct/enum symbol to resolve to a stable `HirDefinitionId` present in the project's `HirTypeRegistry`, and verifies that the registry declaration kind matches the identity kind.
- `TypeEnvironment` now retains the backend-symbol-to-HIR-identity index and materializes `structs_by_id` / `enums_by_id` after declaration registration. Project body checks for struct fields, enum variants, patterns, constructors, and impl receiver types prefer the identity-keyed declaration tables; the string-keyed maps remain for declaration registration and standalone compatibility.
- Nominal struct/enum compatibility checks compare `HirDefinitionId` values first; compatibility-AST symbol equality is retained only for standalone compilation or declarations without project metadata.
- Both `compile_project` and `compile_project_with_metadata` now build the HIR type registry and use the HIR-aware checker after namespace lowering. A missing registry identity is a compilation error instead of silently reverting to name-only nominal checking.
- The LLVM backend now stores project struct layouts by `HirDefinitionId` and enum payload layouts by scoped variant `HirDefinitionId`, with accessors for identity-based lookup. The old string-keyed struct map remains only as a fallback for standalone source programs without module metadata.
- Added tests for distinct same-spelled types across modules, aliases that share one HIR identity, nested container compatibility, and missing/mismatched registry entries.
- Remaining boundary: early type declaration registration and some trait/method tables still use collision-free compatibility symbols, and LLVM instruction/layout emission still consumes the compatibility AST. The identity-keyed layout tables are now available, but replacing all remaining name-keyed lookup and emitting native HIR-ID-based LLVM types is the next migration step. Tests were added but not run because Rust tooling is unavailable in this environment.

### Latest Phase 3 continuation — HIR-keyed method/trait tables and native LLVM layout declarations
- Extended `TypeEnvironment` with `methods_by_id`, `traits_by_id`, and explicit trait/method identity indexes. Project method lookup now uses the receiver's `HirDefinitionId` and does not fall back to the flat compatibility method table once a project identity is known. Trait declaration lookup follows the same rule.
- The type checker now imports the selected method identities produced by module-aware HIR resolution. Inherent methods take precedence over trait-provided methods when populating the executable identity-keyed method table; standalone compilation retains the old string-keyed fallback.
- The LLVM backend now emits actual named LLVM type declarations for project structs and tagged enums, with separately typed enum-variant payload records. Names are assigned deterministically from sorted `HirDefinitionId` values. The current runtime ABI still stores nominal objects behind opaque pointers; constructor/access instruction lowering has not yet been migrated to use these named types directly.
- Struct field HIR identities now map to declaration-order field indices, and enum variant HIR identities map to stable declaration-order discriminants. The backend exposes identity-based accessors and includes the indices/tags in generated IR metadata.
- Added regression tests for HIR-keyed method and trait lookup, LLVM named struct/enum/payload type declarations, field slots, and variant discriminants. These tests are included but could not be executed because Cargo/rustc/rustfmt are unavailable in this environment.
- Remaining Phase 3 work at that milestone: carry the same identities into actual LLVM GEP/load/store and enum construction/match lowering; make trait-implementation selection fully keyed by trait and receiver identity (including multiple traits defining the same method); migrate function/constant symbol lookup; and then remove compatibility string tables where standalone compilation no longer needs them. The GEP/load/store and enum-construction portion is addressed in the following milestone; enum match lowering remains outstanding.
- Validation performed for this increment: the emitted named-type syntax (including empty enum payload records) was independently accepted by the installed Clang LLVM-IR reader. This is only an IR syntax smoke check; it does not compile the Fusion crate or execute the Rust regression tests.


### Latest Phase 3 continuation — HIR-keyed LLVM value operations
- LLVM struct construction now resolves the nominal declaration through its `HirDefinitionId`, allocates storage using the emitted named LLVM struct layout, and initializes fields using field declaration identities and their recorded GEP indices. Constructor lowering rejects missing/duplicate field initializers defensively.
- Struct property reads and property assignments now resolve the receiver's nominal identity, find the field declaration identity, and emit typed LLVM `getelementptr`, `load`, and `store` instructions. The receiver identity is carried through typed parameters, explicit nominal locals/constants, constructor-inferred locals, and nested struct-valued properties.
- Enum construction now resolves the enum and variant identities, allocates the variant-specific payload record, stores each payload value using its declared native storage type, and writes the stable discriminant and payload pointer into the tagged enum layout. Missing payload metadata and incorrect payload arity are rejected.
- Added a tracked LLVM heap-allocation arena for nominal struct/enum/payload values. Main invokes heap cleanup after tuple cleanup, so nominal values returned from helper functions remain valid through the program run instead of pointing into a callee's stack frame.
- Added regression tests for HIR-keyed struct construction/field reads and enum tag/payload construction. An independent Clang IR smoke test accepted the named-type GEP, heap allocator, struct field load/store, and tagged enum construction syntax. Rust tests were added but not run because Cargo/rustc/rustfmt are unavailable.
- Remaining work: enum pattern/match dispatch and CFG lowering, nominal type propagation through arbitrary function return values and collection elements, ownership/escape analysis for heap values, and migration of remaining compatibility-AST symbol paths. The current heap arena is program-lifetime allocation, not a general ownership or garbage-collection solution.

### Latest Phase 3 continuation — HIR-identity-based enum match lowering
- LLVM `Statement::Match` lowering now resolves the scrutinee's nominal enum identity and uses that identity to select the named enum layout and variant declaration metadata.
- Each variant arm compares the stored discriminant against the HIR-keyed variant discriminant and branches through explicit match-arm CFG blocks. Variant payload values are extracted from the corresponding HIR-keyed payload record using its emitted named LLVM layout.
- Direct payload identifier bindings are loaded with their declared native LLVM types and introduced only within the arm's local scope; wildcard payload bindings are ignored. Whole-value identifier and wildcard arms are also supported, as are boolean guards on supported arms.
- Variant patterns are checked against the scrutinee's `HirDefinitionId`, so a same-spelled variant belonging to another module's enum is rejected during lowering rather than matched by spelling.
- Added backend regression tests for discriminant dispatch, payload extraction/binding scope, and rejection of a variant from a different HIR enum.
- At that milestone, nested payload patterns, struct patterns in match arms, scalar/tuple match lowering, and generalized pattern decision trees remained outstanding. The following continuation implements recursive nested enum and struct pattern lowering; scalar/tuple and generalized decision-tree support remain future work. Rust tests were added but not executed in this environment because Cargo/rustc are unavailable.


### Latest Phase 3 continuation — recursive HIR nominal pattern lowering
- Generalized LLVM nominal `match` lowering to accept either enum or struct scrutinees when their stable HIR identity is known.
- Added recursive pattern-test lowering for nested enum variants inside enum payloads, including nested discriminant comparisons and nested payload extraction. Every variant comparison resolves through the owning enum identity and its HIR-keyed discriminant/layout metadata.
- Added struct pattern lowering at top level and inside enum payloads. Selected fields are resolved by owner `HirDefinitionId`, mapped to their HIR field identity and LLVM field index, then recursively tested or bound.
- Added `@` pattern binding support for nominal and scalar values. Identifier bindings are materialized in arm-local storage; nominal bindings retain their HIR type identity. Match-arm locals and tuple/array metadata are restored after each arm.
- Added regression tests for nested enum-in-enum payload patterns, top-level struct patterns, and struct patterns nested inside enum payloads.
- At that milestone, scalar/tuple literal patterns, range/or-pattern decision trees, exhaustiveness analysis, and CFG-aware termination analysis remained incomplete. The current backend still lowers source AST patterns through the HIR identity bridge rather than consuming a fully lowered HIR decision tree.


### Latest Phase 3 continuation — scalar, tuple, range, or-patterns, and CFG termination
- Extended LLVM match lowering beyond nominal patterns to integer, float, boolean, and string literals. String patterns compare contents through `strcmp`; numeric and boolean patterns emit typed comparisons.
- Added integer range decision tests with signed lower/upper-bound checks and inclusive/exclusive upper-bound semantics.
- Added tuple-pattern lowering over Fusion's packed tuple representation. Each element is decoded according to its static type before recursive matching, including nested nominal/tuple values and scalar bindings.
- Added short-circuit or-pattern decision trees. Each alternative gets an isolated compile-time binding environment, successful alternatives converge on the same arm block, and failed alternatives continue to the next alternative.
- Improved type-checker coverage analysis: boolean or-patterns contribute their individual values; enum variants count as covered only when their payload patterns are irrefutable; integer literals/ranges are checked for overlap; duplicate finite-domain patterns and arms after an unconditional/complete finite-domain match are diagnosed as unreachable.
- Corrected function return-flow analysis so a match proves a return only when its unguarded patterns cover the scrutinee's known finite domain (or include an irrefutable arm) and every arm returns. Guarded patterns do not contribute to exhaustiveness.
- LLVM CFG lowering now tracks nested block termination. `if` branches may return independently; merge blocks are emitted only as reachable fall-throughs (otherwise terminated with `unreachable`). Match arms, while/for/for-each bodies avoid emitting backedges after terminating statements, and function fallback returns are omitted when all paths terminate.
- Added regression tests for boolean or-patterns, integer-range comparisons/overlap, tuple element decoding, refutable enum payload exhaustiveness, unreachable arms, guarded-match return analysis, and both-returning `if` branches.
- Representative LLVM IR containing boolean or-pattern branches, float comparisons, string `strcmp`, tuple element decoding, integer range bounds, and returning `if` CFG was accepted by the installed Clang IR reader. This is an independent IR syntax smoke check, not an execution test of the Fusion compiler.
- Remaining limitations: general exhaustiveness for arbitrary integer/float/string domains still requires a wildcard; backend break/continue target stacks and complete HIR-native decision-tree lowering remain outstanding. Rust regression tests are included but not executed in this environment because Cargo/rustc/rustfmt are unavailable.

### Latest Phase 3 continuation — loop-target-aware LLVM control flow
- Added an explicit nested loop-target stack to the LLVM backend. Each active loop records its own `break` destination and `continue` destination, so control statements always target the innermost enclosing loop.
- `while` continue branches to the condition block. Counted `for` and array `for-each` continue branches to dedicated step blocks, ensuring the loop variable/index increment still executes. Break branches directly to the loop exit and skips the step.
- Removed the previous backend rejection for break/continue in `while`, counted `for`, and `for-each` bodies. Nested loops and break/continue inside nested `if`/match control flow use the same target stack.
- Updated CFG termination detection to recognize LLVM `br` and `switch` terminators as well as `ret` and `unreachable`. This prevents the statement emitter from appending instructions after a break/continue branch and lets enclosing `if`/match emitters recognize terminated arms correctly.
- Counted-for and for-each iteration variables now restore any shadowed local and nominal-type binding after the loop, rather than leaking the loop binding into the surrounding scope.
- Added regression tests for while break/continue, counted-for continue-step behavior, nested-loop target selection, and diagnostics for break/continue outside a loop.
- Verification: a representative nested-loop LLVM IR control-flow smoke test was accepted by Clang's IR reader. Rust tests are included but have not been executed because Cargo/rustc are unavailable in this environment.
- Remaining CFG work includes cleanup/defer edges, labelled loops, and more complete control-flow dataflow for values assigned across loop exits.

### Latest Phase 3 continuation — module-aware HIR declaration and body lowering
- Expanded the HIR project model with `HirProject`, `HirModule`, and import metadata. `HirLowerer::lower_project` lowers each source module independently instead of flattening declarations before assigning identity.
- Top-level functions now carry module-qualified `HirDefinitionId` values, generic parameters, async flags, typed parameters, return annotations, and lowered bodies. Inherent and trait implementation methods receive scoped method identities (`module + implementation kind/trait + receiver type + method`).
- Traits, trait method signatures, impl blocks, structs, enums, struct fields, enum variants, and constants now retain stable declaration identities in HIR.
- Completed lowering for `for-each`, match guards, all currently supported recursive pattern forms (variant, tuple, struct, `@`, or, and range), await expressions, conversions, generic call arguments, and method-call arguments. Imports are preserved in both module metadata and HIR statements.
- Multiple variable declarations in one source declaration group now lower into an explicit HIR block instead of being rejected. Function parameters and match/loop bindings are represented in lexical scopes while lowering.
- Added `lower_project_hir(entry)` as a public API returning the module-aware HIR project. Both `compile_project` and `compile_project_with_metadata` now run this HIR lowering stage before the existing semantic/compatibility pipeline, so unsupported AST forms cannot silently bypass HIR lowering.
- Regression tests cover module-qualified function identities, distinct same-spelled functions across modules, scoped inherent/trait method identities, field/variant identities, match guards/or-patterns, and for-each lowering.
- Remaining boundary: HIR lowering now preserves declarations and source structure, but HIR IDs are not yet resolved on every call/constructor/pattern expression, and the type checker/LLVM backend still primarily consume the compatibility AST. The next migration step is to resolve HIR call sites to declaration IDs and move function signature checking/call lowering to those IDs.


### Latest continuation — HIR-resolved call sites and identity-directed LLVM calls

Implemented in the current working tree:
- Added `HirResolvedCall` records to `HirProject`, keyed by source module and call span. Each record stores the authoritative callee `HirDefinitionId`, call kind, receiver identity (for methods), nominal parameter/argument identities, nominal return identity, generic parameter list and async flag.
- Added `resolve_hir_calls(Project)`. It walks nested expressions and statement bodies, tracks known nominal local/parameter types, resolves top-level calls through local/selective/qualified function resolution, and resolves nominal method calls through the receiver's HIR type identity and module-aware method resolver.
- Added signature checks for known declarations: argument count, explicit generic-argument count, and nominal argument compatibility. The pass remains conservative for primitive/generic types whose identities are not yet represented in the nominal type registry.
- `HirLowerer::lower_project` now attaches the resolved call-site table to `HirProject`; project compilation already invokes this lowering pass, so unresolved/invalid known nominal call signatures fail before compatibility lowering continues.
- Added LLVM backend registries mapping compatibility function spellings to HIR declaration IDs and HIR IDs to final backend symbols. The call emitter selects the target through this identity map when registered.
- Added `LLVMBackend::emit_project` and `register_hir_project_functions` as the project-aware bridge that performs HIR call resolution and registers top-level function targets before LLVM emission.
- Added regression tests for resolved function call identities, nominal signature agreement, rejection of mismatched nominal arguments, and LLVM call emission through a registered HIR identity.

Current limitations at the call-resolution milestone:
- Nominal signature checking is only as complete as nominal expression inference; primitive, generic substitution, trait-object and closure signatures still need a richer resolved type representation.
- Existing AST-only `emit_program` remains supported for standalone compilation and uses its compatibility symbol path. Project builds should use `emit_project` to activate HIR-directed call emission.
- Rust regression tests have been added but have not been executed in this environment because `cargo`, `rustc` and `rustfmt` are unavailable.


### Latest Phase 3 continuation — HIR-directed native method-call emission
- Project-aware LLVM emission now registers inherent and trait implementation methods from HIR using deterministic backend symbols derived from each method's full `HirDefinitionId`.
- The project-aware backend maps resolved method-call source spans to their resolved HIR method declarations. LLVM method-call emission selects the target through that identity mapping rather than looking up a method by its source spelling.
- Impl method bodies are emitted as native LLVM functions. The current method ABI passes the receiver as argument zero, followed by the explicit source arguments; this matches the existing Fusion convention where an instance method declares `self` as its first parameter.
- The receiver-first native ABI supports both an explicit first source parameter named `self` and an implicit receiver supplied by the call site. Trait calls use static dispatch to the HIR-selected implementation; dynamic trait objects/vtables are not implemented.
- This implementation is source-edited but not Rust-compiled or test-executed in this environment: `cargo`, `rustc`, and `rustfmt` are unavailable. Clang is installed, but LLVM IR smoke tests alone cannot validate Rust type correctness.


### Latest Phase 3 continuation — trait conformance and receiver signature validation
- Strengthened trait implementation validation so an explicit `self` parameter is treated as the receiver ABI slot rather than as a user-visible trait argument. Implementations may therefore express the receiver explicitly or rely on the implicit receiver convention.
- When an implementation annotates `self`, the annotation must resolve to the implemented receiver type (or `Self`); a different nominal receiver type is rejected.
- Trait conformance now rejects generic or async implementation methods because the current trait declaration model does not encode generic method parameters or async method contracts. Required user parameters and return types continue to be checked using module-aware nominal resolution and `Self` substitution.
- Moved trait conformance validation into `HirLowerer::lower_project` as well as the existing project compilation pipeline, preventing direct HIR/backend entry points from bypassing trait checks. Added regression tests for explicit receiver acceptance and mismatched receiver rejection.
- Remaining type-system gap: resolved call records still preserve nominal identities rather than a unified type representation for primitives, generic applications, tuples, arrays and inference variables. Full generic substitution and primitive argument compatibility are not complete; the legacy checker remains part of the compatibility path.
- Rust tests/build were not run because `cargo` and `rustc` are unavailable in this environment.


### Latest Phase 3 continuation — HIR call-site routing and stricter trait ABI checks
- Project-aware LLVM emission now builds direct call-site target maps for both top-level functions and methods from `HirResolvedCall` declaration identities. The AST expression visitor consumes the call-site target by `(module, source span)` first; including the module prevents equal byte offsets in separate source files from colliding. Compatibility spelling lookup is retained only as a standalone/legacy fallback. This is an incremental bridge toward direct HIR expression emission, not yet removal of the AST visitor.
- Added a backend regression test ensuring a project call emits the HIR-selected target even when the compatibility call spelling is ambiguous or stale.
- Trait signature validation now normalizes optional explicit `self` parameters on both the trait contract and implementation side. Receiver annotations are checked against the implemented nominal type, while generic/async impl methods are rejected until the trait declaration representation can describe those contracts.
- HIR lowering now runs trait conformance validation itself, so project-aware backends cannot accidentally skip it. Added regression tests for explicit receiver normalization and wrong receiver rejection.
- Full type resolution remains incomplete: `HirResolvedCall` currently records nominal identities and generic arity, not a canonical resolved type for every primitive, generic application, tuple, array, or inference variable. That unified type representation and substitution engine are the next type-system milestone.
- The AST expression visitor and compatibility AST-based body emission are still present. The new call-site target maps remove name-based callee selection for resolved project calls, but direct HIR body emission and compatibility-AST retirement remain outstanding.
- Rust build/tests remain unverified because this environment has `clang` but no `cargo` or `rustc`.

### Latest Phase 3 continuation — structural HIR call signatures and generic substitution
- Added the `HirType` algebra for primitive types, void and runtime built-ins, nominal declaration identities, arrays, tuples, iterator/map/option/result/task containers, generic parameters, and generic nominal applications.
- `HirResolvedCall` now retains structural receiver, parameter, argument, and return types alongside the older nominal-only fields during the migration. Existing backends can continue using the compatibility fields while semantic passes consume the richer representation.
- Added structural type parsing/resolution for nested generic arguments, tuple types, and array suffixes. Nominal constructors are represented by HIR declaration identity; generic nominal applications preserve their resolved argument types.
- Call validation now compares known primitive/container/tuple/nominal argument types, infers generic substitutions from arguments, honors explicit generic type arguments, rejects inconsistent repeated generic bindings, and substitutes inferred/explicit bindings into resolved return types. Unknown expression types remain conservative rather than being guessed.
- Trait conformance now compares resolved structural types across module boundaries (including container/tuple structure) before falling back to source spelling for legacy/unresolved types. Existing static HIR trait dispatch and ambiguity diagnostics remain in force.
- Added unit tests for nested generic substitution and inconsistent repeated generic parameters.
- Still outstanding: full generic nominal declaration arity/constraints, inference through every expression/control-flow join, trait associated types/default methods, dynamic dispatch/vtables, and direct HIR-native function/method body emission. The compatibility AST visitor remains active.
- Rust compilation/tests could not be run because this environment has Clang but no `cargo` or `rustc`; these source changes are therefore not build-verified here.
- The next type-inference increment now validates every known array element against a common structural element type (rather than trusting only the first element), infers array/iterator and tuple index result types, resolves primitive struct-field types, handles conversion and `await` result types, and infers arithmetic result types while rejecting incompatible arithmetic operands.
- Nested function and method calls now contribute structural return types, including generic argument substitution and `Self` method returns, so outer generic calls can use those results instead of degrading immediately to `Unknown`.
- Added regression tests for homogeneous/heterogeneous array inference and array-index result inference. These tests are source additions and have not been run because the Rust toolchain is unavailable.

### Phase 3 continuation — structural local type environments

- Added `HirResolvedLocalType` and `HirProject::resolved_local_types`, recording parameter and local binding types by module and source span.
- Added a structural local environment that retains primitive, tuple, array, nominal, generic and container types across local declarations and assignments.
- Array and tuple element inference now sees previously bound local identifiers; array indexing carries the element type into subsequent locals.
- Function and method parameter annotations seed local environments. Generic parameters remain structural `Generic` values until call-site substitution supplies concrete types.
- Assignment validation rejects a known local type changing to an incompatible structural type. Return expressions with known types are checked against declared structural return types.
- Branch-local environments are merged conservatively: a binding survives a join only when all reachable syntactic branches retain the same type. Loop-local updates do not escape the loop scope.
- Added regression tests for primitive/array/index propagation, generic parameter inference through a local call, and incompatible assignment diagnostics.
- These local-type records are now attached to the lowered `HirProject`; the LLVM body emitter has not yet been migrated to consume them. Rust build/tests remain unverified without `cargo`/`rustc`.


### Latest Phase 3 continuation — backend-oriented HIR bodies and explicit CFG
- Added `HirBody` to the project HIR as a parallel backend-facing representation for top-level functions, inherent/trait methods, and module `main` bodies. Each body retains its module and owner declaration identity, local table, entry/exit block IDs, and explicit basic blocks.
- Added stable body-local IDs (`HirLocalId`) and lexical identifier-reference side tables. Identifier expressions point to local IDs by expression-tree path rather than requiring a backend to repeat string-name lookup. Parameters, declarations, loop variables, and match-pattern bindings are represented in the body local table.
- Added structural expression annotations for literals, identifiers, arrays, tuples, indexing, conversions, unary/binary expressions, and awaited task values. Calls and member/property expressions whose exact type cannot yet be reconstructed from the spanless HIR expression node remain `Unknown` rather than being guessed.
- Lowered `if`/`else`, `while`, counted `for`, `for-each`, `match`, `return`, `break`, and `continue` into explicit terminators and successor block IDs. Loop terminators identify their iteration local; match targets preserve patterns and guards. Conditions, loop bounds/iterables, match scrutinees, guards, and return values carry local-reference/type side tables too.
- Added regression tests for branch CFG creation, parameter/local reference identity, structural tuple annotations, and project-level body collection. These tests are included but not executed because `cargo`/`rustc` are unavailable in this environment.
- Migration boundary: this is a backend-consumable HIR CFG model, but LLVM still consumes compatibility-AST bodies. Expression references use stable tree paths because `HirExpression::Identifier` currently discards individual source spans; complete call result types and all operator/field semantics still need a span-preserving resolved-expression layer. The next step is to teach LLVM to consume this body representation for a supported subset, then expand coverage and retire corresponding AST paths incrementally.

### Latest Phase 3 continuation — first direct HIR-to-LLVM emission path

- Added an LLVM backend entry point that consumes `HirBody` directly, without converting HIR operations or expressions back into AST nodes.
- Integrated an incremental direct-emission attempt for ordinary top-level functions; unsupported HIR operations/types fall back to the existing AST emitter.
- Initial direct subset: scalar parameter/local slots keyed by `HirLocalId`, integer/boolean literals, resolved local reads, integer arithmetic and comparisons, boolean `and`/`or`, unary negate/not, local assignments, expression statements, returns, and explicit goto/conditional-branch terminators.
- Loop/match terminators, non-scalar expressions, calls, conversions, nominal/container layouts, and more advanced value merging remain on the compatibility path.
- Added a focused backend test for direct local-ID based HIR expression emission.
- Validation limitation: this environment has no `cargo` or `rustc`; Rust tests and compilation could not be run. Source-level checks and archive integrity checks are not substitutes for compilation.

### Latest Phase 3 continuation — direct HIR loop and scalar match emission

The direct HIR-to-LLVM emitter now handles counted `for` loops and a scalar match-dispatch subset in addition to the existing scalar operations, returns, and conditional branches. Counted loops use the HIR local identity for their induction variable and explicit condition/step labels; HIR back-edges and `continue` targets are redirected through the increment step, while `break` branches to the recorded exit block. Scalar match dispatch supports wildcard, integer, boolean, integer-range, and `or` patterns, with ordered arm dispatch and optional boolean guards. Unsupported foreach iteration, binding/destructuring patterns, nominal patterns, and non-scalar scrutinees still fall back to the compatibility AST emitter.

Two focused backend tests were added for counted-loop CFG emission and scalar match dispatch. Rust compilation and test execution remain unverified in this environment because `cargo` and `rustc` are unavailable; source checks and archive integrity checks are not substitutes for compiling the crate.

### Latest Phase 3 continuation — direct HIR function and method calls
- Preserved source spans on HIR function-call and method-call expressions, then attached the matching `HirResolvedCall` declaration identity and structural signature to expression-tree paths in backend HIR operations and terminators.
- The direct `HirBody` expression emitter now evaluates call arguments in source order and emits LLVM `call` instructions using the HIR-selected declaration-to-backend-symbol mapping. It does not repeat source-name overload or module lookup.
- Direct method calls evaluate the receiver first and pass it as ABI argument zero, followed by explicit arguments. Nominal HIR values use the backend's existing opaque-pointer representation for this scalar-call boundary.
- Added ABI guards: calls fall back to the compatibility AST emitter when the callee has no registered symbol/signature, argument or return types are unsupported, or the resolved HIR signature disagrees with the registered LLVM function ABI. Method bodies attempt direct emission only when their explicit HIR parameter list matches the receiver-first ABI; implicit-receiver bodies remain on the compatibility path for now.
- Added tests for HIR call-reference path attachment, direct identity-selected function calls, and receiver-first direct method calls. These tests are included but have not been executed.
- Remaining limitations: direct HIR calls currently require known scalar/opaque-pointer argument and non-void return types; built-ins with unknown structural signatures, generic instantiations that do not match the registered ABI, async calls, closures/function values, and void-valued call expressions remain on the AST compatibility path. The AST emitter is still retained as fallback.
- Validation: source delimiter checks passed and representative opaque-pointer LLVM call IR was accepted by Clang's IR reader. Rust compilation and regression tests remain unverified because this environment has no `cargo`, `rustc`, or `rustfmt`.


### Latest Phase 3 continuation — direct HIR foreach and scalar pattern bindings

- Extended direct `HirBody` LLVM emission to iterate over arrays represented by Fusion's tracked tuple/array storage. The emitter evaluates the iterable once, reads its length once, uses an explicit index/condition/step sequence, decodes the stored scalar element representation, and writes each element into the loop variable's `HirLocalId` slot. `continue` routes through the increment step and `break` exits the loop.
- Added direct HIR array/tuple value allocation for homogeneous element sequences, including integer, boolean, floating-point, string/pointer values, and local array values. HIR array and tuple locals use the existing pointer ABI. Heterogeneous tuple emission and unsupported element types continue through the AST compatibility path.
- Match targets now carry explicit `HirPatternBinding` metadata (local ID plus a path relative to the scrutinee). Direct scalar match emission supports identifier and `@` bindings and materializes those locals before evaluating a guard, so guards can read the binding through its HIR local identity. Existing scalar literal, range, wildcard and `or` dispatch remains supported.
- Added regression tests for array foreach emission and for pattern-binding initialization before guard evaluation. These tests are included but have not been executed.
- Remaining direct-HIR pattern work: tuple-path projection, struct-field and enum-payload bindings, nested nominal pattern tests, and full or-pattern binding consistency. Iterator protocol lowering, nested/heterogeneous collection representations, and general index/property expressions are also not yet part of this direct subset. The compatibility AST backend remains the fallback for unsupported bodies.
- Validation: source delimiter scans and archive integrity checks passed. Rust compilation and tests remain unverified because this environment does not contain `cargo`, `rustc`, or `rustfmt`.


## Standard-library integration merge (2026-11-06)

The Claude stdlib implementation has been integrated additively into this newer Phase 3 codebase.
The Phase 3 module system, HIR identity metadata, source-root isolation, Foundry package graph,
and package-source cache fingerprinting from this repository remain authoritative.

Integrated runtime areas include TCP/UDP and HTTP/1.1 plus optional HTTPS/HTTP/2 transports,
named time-zone support, a work-stealing task pool, structured task scopes, opt-in concurrent
async execution, and Foundry unit-test/API-doc command integration. TLS and HTTP/2 are optional
Cargo features (`tls` and `http2`); the default build remains dependency-light.

Validation caveat: this merge was performed without an available Rust compiler in the execution
environment. The code and tests have not been compiled or executed here. In particular, verify
`cargo test`, `cargo test --features tls`, and `cargo test --features "tls http2"` on Windows before
relying on HTTPS/HTTP/2. Release panic behavior (`panic = "abort"`) also deserves review for
pool task and test panic isolation.


## Runtime/network continuation (after Phase 3 archive merge)

- HTTP/1.1: added sequential keep-alive handling to the protocol-sniffing HTTP server and a
  reusable client-session API with response framing preservation. Pipelining is not supported.
- HTTP/2: added a client session API for multiple in-flight streams and explicit server push via
  `serve_with_push`. This is an incremental protocol implementation, not yet a fully concurrent
  server scheduler or complete RFC-conformance implementation.
- Timezones: bundled IANA tzdata 2025b TZif files as an embedded fallback, preserving historical
  transitions on systems without a system timezone database.
- Preemptive green threads and truly suspending async/await with async I/O remain unimplemented;
  see `REQUESTED_RUNTIME_COMPLETION_STATUS.md` for the required architectural work.


### Latest Phase 3 increment — package-scoped multi-module linking

- Foundry now caches package-scoped LLVM modules emitted with `emit-package-llvm`, using the application entrypoint as the common module-identity context so package-qualified symbols agree across units.
- Added a separate `emit-entrypoint-llvm` output containing `main`, runtime support and runtime state, with package-owned functions represented as external declarations.
- Shared runtime helper functions and their state globals now have external linkage in the entrypoint unit. Package modules declare the runtime helpers they use rather than duplicating runtime state.
- `link-llvm` accepts multiple IR inputs, and Foundry links the entrypoint/runtime unit with all package units. Package cache records were bumped to format v4 / `llvm-package-ir-v2` to invalidate earlier whole-project artifacts.
- Empty package modules are supported for unused dependencies and packages with no package-level functions, so a manifest dependency or main-only application does not require invented function definitions.
- Package/global data ABI and full typed ABI validation remain incomplete. Generic, async, aggregate and inferred cross-package signatures still require explicit ABI support. Transitive package source-root integration also needs further validation.
- Validation: a local Clang smoke test successfully linked two separate LLVM IR files with an external package function and shared runtime helper. Rust compilation/tests remain unverified because this environment does not provide `cargo` or `rustc`.


### Latest Phase 3 increment — scalar package data globals

- Added compiler lowering for module-owned integer, floating-point, boolean, and string literal constants as stable LLVM global definitions. Constant references resolve through their HIR declaration identity and become loads from those globals; lexical locals continue to shadow constants correctly.
- Package-scoped LLVM modules now define their own globals, externalize referenced globals from other packages, and retain private string-literal storage. The entrypoint emits external data declarations for referenced package globals, preventing duplicate definitions.
- Package interface format advanced to v4 with public `abi-data` and private `abi-internal-data` records. Foundry verifies each record against the real LLVM global definition and checks cross-package scalar type and visibility before linking. Cache kind advanced to `llvm-package-ir-v4`.
- Added regression tests for compiler data emission and Foundry's data-provider, type-mismatch, and private-data checks.
- Remaining data ABI work: non-literal and mutable globals, struct/enum layout emission, aggregate values, and richer method/generic ABI support.
- Validation: Clang accepted and linked representative separate LLVM modules with `i64`, `i1`, and pointer-valued constant globals. Rust tests remain unexecuted because this environment does not provide `cargo` or `rustc`.


### Phase 3 continuation — stable aggregate ABI (2026-11-11)

The package LLVM path now assigns nominal struct/enum identified types from stable module/type identities rather than order-dependent counters. Package interface v5 publishes matching struct layout symbols and ordered field storage types. Foundry validates each public struct interface against the concrete LLVM identified type in its package unit and rejects conflicting layouts for the same type identity across package modules and the entrypoint. Package cache metadata was advanced to record format v6 / `llvm-package-ir-v5` to prevent reuse of artifacts generated with the old type naming scheme.

Supported cross-package struct layouts currently include scalar fields and pointer-represented nominal fields. Arrays and other unsupported aggregate field storage are still rejected explicitly; aggregate-by-value function signatures and full enum payload ABI remain future work. Rust tests were added but not executed because this environment does not have `cargo` or `rustc`. A Clang multi-module smoke test confirmed that matching stable identified struct layouts link and execute correctly.


## Native cross-package aggregate execution test

- Extended the three-module aggregate ABI regression fixture beyond textual LLVM assertions. When Clang is available, the test writes the actual independently emitted `model`, `consumer`, and application-entrypoint LLVM modules, links them into a native executable, runs it, and checks the output.
- The fixture exercises a provider-created struct returned by pointer and read by a consumer package, plus both payload-bearing enum variants constructed in the consumer package and matched in the application: an integer payload (`Success`) and a string payload (`Failure`).
- Added explicit checks for both variant payload identified-type layouts and the imported string-payload enum constructor ABI.
- This is an executable compiler/backend integration test, but it has not been run in this environment because `cargo`/`rustc` are unavailable. Clang is installed; the test will perform the native link and execution when run under the Rust test harness. Foundry's production build path already runs `validate_package_link_inputs` (including struct/enum/payload layout checks) before invoking the compiler's `link-llvm` command.

### Phase 3 continuation — aggregate ABI pre-link rejection coverage (2026-11-11)

- Added Foundry link-gate regressions for a public struct whose LLVM identified layout drifts from its interface and for an enum whose concrete payload identified layout drifts from its payload ABI record.
- Both tests call `validate_package_link_inputs`, the exact validation entrypoint invoked by the production `build_inner` flow before `link-llvm`. The intended behavior is rejection before the native linker is started.
- This closes the test-coverage gap between isolated struct/enum layout validators and the full pre-link validation entrypoint. It does not yet constitute an executed Foundry build using a Rust-built Fusion compiler.
- Rust tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable in this environment.


## Phase 3 increment — package artifact cache corruption recovery

- Added a Foundry compiler-harness regression that drives `compile_package_artifacts` with a deterministic fake Fusion compiler. It produces package LLVM and v6 interface artifacts through the same command interface used by the build orchestrator.
- The test establishes a cache record, verifies a valid artifact/interface pair is reused without invoking the compiler again, then independently corrupts the cached LLVM bytes and the cached interface bytes. Each corruption must invalidate the cache entry and regenerate the package artifacts before they can reach the pre-link validation gate.
- The test checks compiler invocation counts across the cold build, warm cache hit, LLVM corruption recovery, and interface corruption recovery. This covers recovery behavior rather than only checking that a checksum mismatch is detectable.
- This remains a focused orchestration-layer test, not a complete `build_inner` invocation: dependency resolution, entrypoint generation, ABI pre-link validation, native linking, and executable runtime are still separate integration layers.

**Validation limitation:** the Rust regression has been added but could not be executed here because `cargo`, `rustc`, and `rustfmt` are unavailable. The archive and Cargo manifest checks are performed separately and do not establish that the Rust test compiles or passes.

### Phase 3 increment — package cache commit safety

Foundry now prevalidates the complete active package artifact/interface set before publishing cache records, stages records before renaming them into the cache, and writes package fingerprints and the executable success stamp only after package artifacts and graph state have been persisted. A regression test verifies that a malformed interface in a later package cannot leave an earlier package's record or successful package state behind. This reduces partial-commit risk but does not provide a filesystem-wide atomic transaction across several record/state files. Rust test execution remains pending a Rust toolchain.


## Phase 3 increment — full Foundry build orchestration regression coverage

- Added Unix Foundry integration fixtures that invoke the production `build_inner` orchestration with a deterministic fake Fusion compiler and a real two-node path-dependency graph (`model` followed by `app`). The fixture exercises package graph resolution, dependency-first package compilation, package-interface generation and validation, entrypoint artifact generation, package-link input validation, the `link-llvm` command boundary, and successful package/graph/fingerprint state publication.
- The successful-build test invokes `build_inner` twice. It verifies that the first invocation compiles both package modules and the entrypoint, invokes the linker, and commits all package records and state. The second invocation reuses the package cache and matching executable fingerprint without recompiling or relinking.
- Added a malformed-interface case that verifies the build stops before invoking `link-llvm` and does not publish successful package/graph/executable state.
- Added a native-link failure/retry case. The first link attempt fails and must leave package artifact records, package fingerprints, graph state, and the executable success stamp uncommitted. After the fake linker is allowed to succeed, a retry rebuilds the uncommitted package modules, links successfully, and publishes the expected state.

**Scope and validation boundary:** these are full `build_inner` orchestration tests, but they use a fake compiler/link command and therefore do not prove that a Rust-built Fusion compiler emits executable LLVM for this fixture. The path-dependency graph is real; registry/network dependency resolution is not exercised (`resolve=false`). Rust tests remain unexecuted in this environment because `cargo`, `rustc`, and `rustfmt` are unavailable.


## Phase 3 continuation — incremental graph invalidation and last-known-good executable safety

- Native linking now targets a unique sibling staging file instead of the currently published executable. A failed linker, a missing staged output, or a failed package-state commit cleans up the staging file and leaves the previous executable untouched. On successful linking, package records and graph/fingerprint state are committed before the staged executable replaces the published output; the executable fingerprint stamp is written atomically last.
- Remote-cache restoration also downloads to a staging path, so a failed or incomplete restore cannot truncate the last known-good executable. Successful restore follows the same metadata, executable replacement, and final-stamp ordering.
- Added a `build_inner` incremental regression using a real two-package path-dependency graph and the fake compiler harness. Editing the dependency invalidates its own key and the root key through dependency-aware fingerprint propagation. A forced link failure must preserve the prior executable bytes, build stamp, package fingerprint state, graph snapshot, and committed dependency record. A retry rebuilds the uncommitted package artifacts and succeeds.
- The same test removes the dependency from the root manifest, reloads the project, rebuilds, and verifies that only the remaining root package is recompiled and the removed package's stale cache record is collected after successful linking.
- Validation performed for this archive: ZIP integrity, Cargo manifest TOML parsing, shell syntax for the fake compiler fixture, and source-level assertions. Rust compilation and tests were not run because `cargo`, `rustc`, and `rustfmt` are unavailable in this environment. These tests are added to the real Foundry `build_inner` path but still use a deterministic fake compiler/link command; they do not substitute for a real Rust-built Fusion compiler end-to-end run.


## Phase 3 continuation — reproducibility, cache isolation, and interrupted-publication recovery (2026-11-12)

- Added a pair of independently rooted but byte-identical two-package projects using the same compiler identity. Their dependency-aware package keys match, and the fake compiler produces byte-identical package LLVM and interface artifacts. Their cache stores remain physically separate; corrupting one project's artifact does not invalidate or trigger recompilation in the other project's warm cache. This is scoped reproducibility coverage for identical inputs/compiler identity, not a claim of real-compiler bit-for-bit reproducibility across toolchains or hosts.
- Added concurrent `build_inner` invocations for two distinct `--target` output directories. Both complete with independent executable stamps, package fingerprint state, graph state, and package artifact stores. This covers concurrency across isolated target directories; concurrent writers to the exact same output directory remain outside this test.
- Added Debug/Release isolation coverage. The profile participates in package fingerprints, and each profile maintains separate artifacts, package state, and executable success stamps.
- Added an interrupted package-record publication scenario by placing a filesystem object at a deterministic staged-record path. The build must fail without changing the last known-good executable, build stamp, package fingerprint state, or graph snapshot. Removing the stale object allows the next build to recover and publish successfully.
- Validation in this environment: ZIP integrity, both Cargo manifest TOML parses, and shell syntax checks for the fake compiler fixture. Rust compilation/tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. All newly added integration scenarios use the production Foundry orchestration with a deterministic fake compiler/linker; real Fusion compiler/native runtime verification remains outstanding.


## Phase 3 continuation — compiler identity and build provenance (2026-11-12)

- Package fingerprints now include the resolved compiler path, SHA-256 of the compiler executable bytes, compiler `--version` output and exit status, target OS/architecture, profile, package inputs, and transitive dependency keys. The package fingerprint domain was advanced to `foundry-package-key-v2`; the root build fingerprint domain is `foundry-build-key-v2`. Replacing a compiler executable in place now invalidates package artifacts even if its path and reported version are unchanged.
- The Fusion CLI now supports `--version`/`-V`/`version`, reporting the Cargo package version. This makes compiler version discovery explicit and machine-readable for Foundry.
- Successful local and remote-restored builds write `.foundry-build-provenance.toml`, recording provenance format version, compiler path/hash/version, package interface version, profile, package fingerprints, package LLVM artifact hashes, and final executable hash. The executable success stamp is still written last.
- Fresh-build reuse now requires valid provenance whose executable SHA-256 matches the current executable; a missing, malformed, or mismatched provenance file forces a rebuild/relink rather than trusting the stamp alone.
- Added regression coverage for compiler binary replacement at a stable path/version, provenance contents, and tampered provenance forcing a fresh link.
- Validation in this environment: archive integrity, Cargo manifest TOML parsing, source-level assertions, and fake compiler shell syntax checks. Rust compilation/tests and a real Fusion-generated native executable remain unverified because `cargo` and `rustc` are unavailable. Compiler executable hashes cover the Fusion binary itself; system linker/Clang identity and dynamically loaded toolchain dependencies are not yet included in the provenance contract.

- Provenance also records the root build fingerprint and the entrypoint LLVM artifact hash; fresh-build reuse requires the stored provenance build fingerprint to match the current expected key.


## Phase 3 continuation — linker identity, ordered LLVM inputs, and remote provenance (2026-11-12)

- Foundry now resolves Clang using the same precedence as Fusion's `link-llvm` implementation (`FUSION_LLVM_DIR`, `LLVM_HOME`, `PATH`, then the supported Windows locations). The resolved linker path, executable SHA-256, `--version` output, and version-command status participate in the root executable fingerprint. Linker changes therefore force a relink without unnecessarily invalidating package LLVM artifacts. The compiler and linker are distinct provenance identities: Foundry invokes the Fusion compiler's `link-llvm` subcommand, which in turn invokes Clang.
- Provenance format advanced to v2. It records linker path/hash/version, the ordered link input list (entrypoint first, followed by package LLVM modules in dependency-first order), each input's SHA-256, and a digest over that ordered list.
- Local fresh-build reuse now compares the complete expected provenance document against the stored document, not only the executable hash. Altered linker identity, input order, or input hashes force a fresh link.
- Remote cache publication now uploads a `.provenance.toml` sidecar before publishing the executable object. Restoration requires both objects and compares the downloaded sidecar semantically against the locally expected compiler/linker identities, package/interface fingerprints, ordered LLVM input hashes, and downloaded executable hash. Missing, malformed, or mismatched sidecars are treated as remote cache misses; unverified executables are discarded and the local link path proceeds.
- Added regression coverage for linker/input provenance comparison and for tampered linker provenance forcing a relink. Existing provenance assertions now expect format v2.
- Validation limitation: Rust compilation/tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. Clang is present at `/usr/local/swift/usr/bin/clang`, but the actual Rust-built Fusion compiler and its native end-to-end executable path are still not run in this environment. The linker identity currently fingerprints the Clang executable and version output; dynamically loaded shared libraries, SDK contents, and implicit environment configuration are not independently hashed.


## Phase 3 continuation — linker target/resource provenance and remote-cache HTTP integration (2026-11-12)

- Root build fingerprint domain advanced to `foundry-build-key-v3`. Linker identity now includes Clang's reported target triple (`-dumpmachine`), resource-directory path (`-print-resource-dir`), a deterministic SHA-256 over regular files and relative paths beneath that resource directory, and the effective linker flag list (`-O2`). The same fields are included in the provenance document, advanced to format v3. This detects changes to builtin Clang headers/resources even when the Clang executable is unchanged. It does not yet hash the host SDK, system linker libraries, environment variables, or all dynamically loaded dependencies.
- The Fusion `link-llvm` implementation now names its effective `-O2` linker flag in one local constant, with an explicit synchronization comment tying it to Foundry's provenance contract. The existing link invocation behavior is unchanged. Foundry's `--target` remains an output/cache namespace selector in this code path; it is not represented as a Clang cross-compilation flag. The actual Clang target triple is recorded separately.
- Added a local TCP HTTP server test harness in `foundry/src/cache.rs`. Integration tests exercise real `curl` PUT/GET operations for the provenance sidecar and executable object, verify their exact stored paths and bytes, and verify that a missing sidecar returns a cache miss and leaves no partial destination file.
- Added a linker identity regression test proving that a Clang resource header change and a reported target-triple change both alter the linker identity fields. Provenance assertions now require target triple, resource directory/hash, and effective linker flags.
- Validation in this environment: source inspection and archive/TOML checks are available; Rust compilation/tests remain unexecuted because `cargo`, `rustc`, and `rustfmt` are unavailable. The local HTTP integration tests are implemented but have not been run here.


### Phase 3 continuation — linker search-path and environment provenance (format v4)

- Extended the Clang identity used by Foundry fingerprints with `-print-search-dirs` output and its digest. This records the compiler driver's discovered program/library search configuration in addition to its binary, version, target triple, and resource directory.
- Captured a bounded allowlist of toolchain-affecting environment variables (SDK selection, Clang config overrides, compiler/library/header search paths, dynamic-library paths, and `SOURCE_DATE_EPOCH`). Set and unset states are both explicit and contribute to the root executable fingerprint.
- Build provenance format advanced from v3 to v4 and now stores the linker search-directory output/hash and the allowlisted environment values. Local freshness validation rejects older provenance formats and compares the complete expected document.
- Added regression assertions for search-directory identity, environment capture, and provenance differences caused by changed search paths or SDK environment values.
- The actual Fusion compiler/native multi-package execution milestone remains unverified: this environment still has no `cargo` or `rustc`, so Rust tests cannot be run here.

### Phase 3 continuation — real toolchain smoke harness and auditable native fixture (2026-11-12)

- Added a checked-in three-package Foundry workspace fixture under `tests/fixtures/phase3-cross-package-foundry`: `model` exports a struct and payload-bearing enum, `consumer` depends on `model` and transfers/reads those nominal values across the package boundary, and `app` depends on both packages and exercises enum payload matching.
- Added `scripts/phase3-real-toolchain-smoke.sh`. It checks for Cargo, rustc, Clang, and Python 3.11+; builds the actual Fusion compiler and Foundry binaries; runs the compiler's native cross-package aggregate ABI integration test; runs Foundry's build-orchestration regression test; then invokes the real Foundry CLI against the fixture.
- The smoke harness executes the produced native program and requires output `7`, `9`, `failed`. It then parses the generated format-v4 provenance with Python `tomllib`, verifies compiler/linker/executable SHA-256 values, checks all three package artifact and interface records against their hashes, verifies ordered dependency-first link inputs, and runs a second build to check that warm-build output bytes remain stable.
- The script supports `PHASE3_KEEP_WORKSPACE=1` to retain the generated temporary project for debugging. It is a Bash/Linux-or-macOS developer harness, not a platform-neutral test runner.
- Validation in this environment: Bash syntax, fixture TOML parsing, Python verifier syntax, and archive integrity. The script itself could not be executed because `cargo` and `rustc` are not installed here. Consequently no claim is made that the real Rust-built compiler, Foundry CLI, native executable, or provenance verifier has passed end-to-end in this environment; the harness is now checked in to make that verification reproducible on a Rust-equipped host.

### Phase 3 continuation — required real-toolchain CI gate

- Added `.github/workflows/phase3-native-integration.yml`, a GitHub Actions workflow that runs on relevant pushes and pull requests and supports manual dispatch. Its single `required-phase3-gate` job installs stable Rust, Clang, curl, and Python; checks formatting for both Rust crates; runs all targets for the Fusion and Foundry crates; then runs the real compiler/Foundry/native execution/provenance smoke harness.
- The workflow uploads a 30-day evidence artifact even when a preceding step fails. Evidence includes toolchain versions, the full smoke log, run status, and—when generated—the retained fixture workspace, native executable, package cache records, and provenance document.
- Extended `scripts/phase3-real-toolchain-smoke.sh` with optional `PHASE3_ARTIFACT_DIR` support. It preserves the integration workspace and records run metadata/exit status in that directory, while retaining its existing `PHASE3_KEEP_WORKSPACE` behavior.
- Added `docs/phase3-ci.md` with the gate's scope, evidence contents, and the exact branch-protection status-check name: `Phase 3 native integration / required-phase3-gate`. Repository branch protection must still be enabled by an administrator in GitHub settings; it cannot be imposed by committing the workflow alone.
- Validation in this environment: workflow/script structure and shell syntax are checked, Cargo manifests parse as TOML, and ZIP integrity is checked. The GitHub Actions job has not been executed here, and this environment has no `cargo` or `rustc`; therefore Rust tests and real native execution remain pending the first CI run.


### Phase 3 continuation — Debug/Release native execution and cache isolation

- The real-toolchain smoke harness now builds and executes the three-package struct/enum fixture in both Debug and Release profiles. Both binaries must produce the same expected output.
- The Release build is required to leave the previously built Debug executable and Debug provenance document byte-for-byte unchanged. Release provenance is independently checked for the expected profile, compiler/linker identity fields, ordered link-input digest, package interface version, and executable hash.
- Both Debug and Release warm builds are checked for executable-byte stability. The GitHub Actions required Phase 3 gate runs this expanded smoke harness and retains both profile outputs and provenance files when available.
- This is test-harness and CI-gate implementation, not evidence of a passing real native build yet. Rust compilation and native execution remain pending the first run on a Rust-equipped CI runner.


### Phase 3 continuation — locked dependency source identity and provenance v5 (2026-11-13)

Foundry lockfiles are now format v3 and persist the registry origin for every registry package. Builds reject manifest drift, registry-origin drift, changed path dependency content, and unpinned path dependencies in offline mode until `foundry update` refreshes the lock. Lockfile publication uses unique staging files and atomic rename. Workspace registry consistency is scoped by registry origin and alias. Build provenance format v5 records per-package lockfile SHA-256 values, and the real-toolchain smoke verifier checks them. New regression tests cover these behaviors, but Rust compilation/test execution remains pending because this environment has no Rust toolchain.

### Phase 3 continuation — registry extracted-cache integrity

Registry resolution now verifies extracted package trees against a deterministic local tree manifest bound to the locked archive SHA-256. Modified or incomplete extracted trees are rebuilt from the checksum-verified archive, including in offline mode. Archive path traversal, absolute paths, links, and special entries are rejected before extraction. Re-extraction is staged and restores the previous extracted directory if publication fails. Tests were added for tamper repair, sidecar corruption, malformed archive preservation, invalid cache names, and symlink rejection. Rust tests are not claimed as passing because this environment has no Rust toolchain. See `foundry/docs/registry-cache-integrity.md`.

Registry cache lifecycle now includes per-package process locks, stale-lock recovery, transaction-artifact cleanup, and a workspace-scoped `foundry cache prune` command that preserves validated lockfile pins.


## Registry cache observability

Foundry now exposes read-only registry cache diagnostics through `foundry cache inspect` and JSON output through `foundry cache inspect --json`. The report distinguishes metadata, archive checksum, extraction, and extracted-tree integrity states. Inspection does not mutate cache entries; recovery remains part of normal verified package resolution, and pruning remains a separate explicit operation. Rust compilation and tests require a Rust toolchain and have not been executed in this environment.


## Phase 3 continuation — registry protocol hardening

Registry metadata is now explicitly versioned (format 1) with mandatory exact identity and archive/checksum fields. Network access is HTTPS-only with HTTPS-only redirects, bounded request timeouts, and clearer failure diagnostics. Unsupported URL schemes, credentials, fragments, and relative archive locations are rejected; absolute local paths and `file://` remain available for local registry use. Registry index/metadata cache writes use staging and atomic publication, with metadata validation before replacing cached records. Added schema, URL-policy, local-registry range-resolution, and last-known-good metadata tests. These Rust tests have not yet been run in this environment because the Rust toolchain is unavailable; remote HTTPS interoperability remains a CI verification item.


### Phase 3 integration-readiness audit (2026-11-13)

- Audited `.github/workflows/phase3-native-integration.yml`, `scripts/phase3-real-toolchain-smoke.sh`, the three-package fixture, and `docs/phase3-ci.md` against the current provenance and lockfile formats.
- Strengthened the Release-profile provenance verifier to check compiler path/hash, linker existence/hash and identity fields, build fingerprint, package artifact and interface records, package lockfile hashes, ordered package link inputs and their digest. Debug and Release now have equivalent core provenance integrity assertions before Release/Debug isolation is accepted.
- Corrected the fixture README, which still described provenance format v4 while the current harness requires format v5.
- Added an explicit Phase 3 acceptance checklist and completion boundary. It distinguishes implemented assertions from evidence still pending a Rust-equipped CI run, and calls out branch-protection configuration as an administrator action.
- This is a readiness/audit increment, not a claim that Phase 3 has passed. Rust compilation, test execution and real native integration remain unverified in this environment because `cargo`, `rustc` and `rustfmt` are unavailable. The roadmap still records HIR authority, unified type resolution/generic substitution, remaining ABI coverage and type-level namespace work as open scope decisions.
