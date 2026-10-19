This folder(Shared ecosystem infrastructure) is downloaded, whenever an ecosystem plugin is required. Inside of it, are 3 folders.
The first folder is "toolchain-orchestrator", the second "static analyser", third "fusion-interop-model".
Below are the former readme.md's for the relevant folders.

------------------------------------------------------------------------------------------

# Fusion Toolchain discovery and foreign-build orchestration.

The orchestrator owns external compilers, linkers, SDKs and runtimes. It
consumes the neutral `fusion-interop-model` rather than putting foreign
toolchain details into the Fusion parser or Foundry dependency resolver.
Coordinates plugin availability with builds, tests, analysis and other development tools.

Toolchain Orchestrator
Coordinates analysis, builds, tests, compatibility checks and any migration workflow.

Core responsibilities of the toolchain orchestrator include:
Workflow coordination
Coordinates the compiler, static analyser, formatter, test runner, debugger and other tools, choosing the correct execution order and managing dependencies between tasks.

Build pipeline management
Coordinates compilation stages, conditional builds, incremental compilation, linking and artifact generation.

Tool and plugin integration
Discovers installed tools and plugins through Foundry, checks compatibility and invokes them through defined interfaces.

Resource and execution management
Manages parallel tasks, caching, resource limits, task cancellation and execution timeouts.

Result aggregation
Collects compiler errors, test results, static analysis findings, compatibility issues and performance reports into one unified result.

Environment and compatibility coordination
Ensures tools, plugins, targets and language versions are compatible before execution, and reports unmet requirements.

-----------------------------------------------------------------------------------------

# Fusion Static Analyser

A small, dependency-free Rust crate scaffold for static analysis in Fusion.

## Layout

- `analyzer.rs` — runs registered analysis rules over a source file.
- `config.rs` — analyser configuration.
- `diagnostic.rs` — source locations and findings.
- `rule.rs` — common interface for analysis rules.
- `rules/` — individual rules.

The initial rules are deliberately conservative examples. They operate on a small
`SourceFile` abstraction rather than assuming Fusion's parser/AST API. Once Fusion's
AST is exposed as a library, rules can be upgraded to consume typed AST nodes and
symbol/type information.

## Use

```rust
use fusion_static_analyser::{Analyzer, SourceFile};

let analyzer = Analyzer::default();
let source = SourceFile::new("main.fus", "fn main() {\n    let unused = 1;\n}\n");
let diagnostics = analyzer.analyze(&source);

for diagnostic in diagnostics {
    println!("{diagnostic}");
}
```

the static analyser should identify opportunities and provide evidence, while a separate component decides whether to recommend a language transition and generates the message to the developer.

Suggested architecture:

Fusion Source Code
Application and dependencies

Fusion Performance Analyser
Detects bottlenecks, resource usage and optimization opportunities

Interoperability Advisor
Evaluates whether another language or implementation could offer a measurable benefit

Recommendation Generator
Produces an actionable explanation for the developer

Static Analyser
Inspects code for inefficient patterns, allocations, unnecessary copies, and expensive operations.
Combines static analysis with profiling and benchmarking to identify actual bottlenecks.
Evaluates whether moving a specific module to another language could help, considering the five interoperability levels.
Creates the human-readable message, including evidence, estimated benefits, trade-offs and suggested next steps.


Example of the message Fusion could generate:

Performance opportunity detected
Your image_processing() function accounts for 38% of the application's measured execution time and performs frequent memory allocations.
Suggested option: Consider implementing this module in Rust through Fusion's interoperability system.
Potential benefit: Lower allocation overhead and improved execution speed, subject to benchmarking.
Memory impact: May reduce temporary allocations, depending on the implementation.
Compatibility: Requires checking the module's data types and interoperability boundary.
Trade-off: Additional maintenance and cross-language integration complexity.
Recommendation only. Benchmarking and developer approval are required before migration.


The important part is that Fusion should not automatically assume another language will be faster. A language change can introduce FFI overhead, data conversion costs, memory ownership complications or additional maintenance. The analyser should distinguish between a potential performance gain and one demonstrated by benchmarks.

Where the five interoperability levels fit

I would make the Interoperability Advisor aware of Fusion's five levels, so it can recommend the least disruptive level that meets the requirement:
Low interoperability: Call an external component through a simple interface.
Higher interoperability: Share richer types, data and control flow.
Deep interoperability: Integrate more tightly with another language's runtime or memory model.

The exact descriptions should follow your planned five-level specification, but the general principle is to recommend only as much integration as necessary.

------------------------------------------------------------------------------------------

# Fusion interoperability model.

This crate defines the language/toolchain-neutral vocabulary for crossing the Fusion/foreign boundary. It deliberately contains no compiler process management. Foundry resolves *what* a project needs; 

the toolchain orchestrator decides *how/with which tools* to build it.

There are 5 layers of interoperability:
1. Native ABI: the foreign code is compiled into a static or shared library and linked into the Fusion binary. The foreign code is called directly from Fusion code, and vice versa, using the platform's native ABI.
2. Managed runtime: the foreign code is compiled into a library that is loaded into the Fusion process, and called via a managed runtime (e.g. JVM, CLR, etc.). The foreign code is called from Fusion code via the managed runtime's interop facilities, and vice versa.
3. Embedded runtime: the foreign code is compiled into a library that is loaded into the Fusion process, and called via an embedded runtime.
4. Process: the foreign code is compiled into a separate executable that is launched by the Fusion process. The foreign code is called from Fusion code via inter-process communication (IPC), and vice versa.
5. WebAssembly: the foreign code is compiled into a WebAssembly module that is loaded into the Fusion process, and called via the WebAssembly runtime.