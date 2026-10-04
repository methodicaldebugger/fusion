# TypeScript Interoperability Plugin

TypeScript interoperability can connect Fusion to the JavaScript ecosystem,
including web libraries, Node.js packages, backend services, and developer
tools.

TypeScript is normally type-checked and then transpiled to JavaScript. Its
types are generally erased at runtime, so typed Fusion bindings require
declaration files, generated metadata, or explicit bridge definitions.

## Initial integration strategy

Start with a Node.js-based runtime and transpile TypeScript to JavaScript:

    Fusion -> JavaScript bridge -> Node.js runtime -> JavaScript package

Foundry should eventually manage the Node.js version, TypeScript compiler,
npm dependencies, and reproducible build/run configuration.

The initial boundary should support a small set of predictable values, such
as booleans, numbers, strings, and structured data with defined conversions.
Promises should be mapped deliberately to Fusion's asynchronous model.

## TypeScript-specific concepts

A richer integration may need to account for:

- TypeScript transpilation and compiler configuration
- Node.js runtime lifecycle
- JavaScript values and dynamic types
- Type erasure and `.d.ts` declaration files
- Promises and Fusion `async`/`await`
- JavaScript exceptions and Fusion `Result` conversion
- npm dependency resolution and module formats
- Callbacks, event loops, and data conversion

TypeScript's compile-time types do not by themselves guarantee runtime
validation or safe conversion across the Fusion boundary.

## Future direction

A TypeScript-aware binding generator could use declaration files to produce
Fusion-facing interfaces, with runtime checks where necessary. Other
JavaScript runtimes could be considered after the Node.js integration is
stable.

## Status

Scaffold only. No transpilation, Node.js embedding, binding generation,
package management, or invocation is implemented.
