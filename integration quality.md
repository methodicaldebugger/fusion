The integration quality of foreign languages and foreign libraries may vary:

Tier 1 — Native integration
C, C++, Rust

Tier 2 — Runtime integration
Java, C#, Dart, Kotlin

Tier 3 — ABI/binding integration
Swift, Go, Zig

Tier 4 — External bridge
Python, Ruby, Julia, etc.

Tier 5 — WebAssembly/RPC
Almost anything capable of exposing a suitable interface


Imagine Fusion eventually becomes good enough that you can say:

You don't need to rewrite your 20-million-line Java/C++/C# codebase. Keep it. Write new software in Fusion and consume the existing systems.

A tutorial written for Python, Rust, Java, C#, C++, or another ecosystem should become transferable knowledge.


1. Fusion the language(version 1.0): 
THIS IS STILL AN OPEN-SOURCE PROJECT. 
We are developing: syntax, parser, type system, interpreter, compiler, Fusion IR, LLVM backend, garbage collector, standard library, package manager, debugger, language server, formatter, documentation. Until fusion reaches self-hoasting.

2. Fusion the interoperability platform(version 1.0-1.2):
THIS IS WHERE I STOP HELPING
Once C interoperability has been achieved, fusion is completed from my perspective.

3. Fusion 1.2-infinity:
JIT + AOT, repl + jupyiter.
Now we implement more languages and foreign libraries(the developer can use foreign libraries, but may only write the foreign language itself, if there is a plugin available).

We implement many plugins, that allow the developer write other programing languages. C++, ABIs, Rust, JVM/Java, .NET/C#, Swift/Objective-C, Go, Dart, Kotlin/Native, Zig, WebAssembly, different runtimes, different memory models, different build systems, package distribution, licensing, security, debugging, cross-platform deployment.

We don't necessarily need the companies behind every language to participate initially, but their ecosystems become important.

At this point languages are just libraries to fusion
"Fusion lets you consume capabilities implemented in 10+ different languages."
But Fusion should be designed so that 10 doesn't become a ceiling.
Not all of those need equal integration quality.