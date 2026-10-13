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

Suggested architecture

step 1. Fusion Source Code
Application and dependencies

step 2. Fusion Performance Analyser
Detects bottlenecks, resource usage and optimization opportunities

step 3. Interoperability Advisor
Evaluates whether another language or implementation could offer a measurable benefit

step 4. Recommendation Generator
Produces an actionable explanation for the developer



Toolchain Orchestrator coordinates the workflow; Foundry manages builds, dependencies, tests and publishing.



What each component should do

Component
Responsibility

Static Analyser
Inspects code for inefficient patterns, allocations, unnecessary copies, and expensive operations.

Performance Analyser
Combines static analysis with profiling and benchmarking to identify actual bottlenecks.

Interoperability Advisor
Evaluates whether moving a specific module to another language could help, considering the five interoperability levels.

Recommendation Generator
Creates the human-readable message, including evidence, estimated benefits, trade-offs and suggested next steps.

Toolchain Orchestrator
Coordinates analysis, builds, tests, compatibility checks and any migration workflow.

Foundry
Supplies dependency resolution, build tooling, test execution, compatibility management and publishing.





Example of the message Fusion could generate

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


The analyser detects. The advisor evaluates. The recommendation generator explains. The developer decides. The orchestrator executes only after approval.