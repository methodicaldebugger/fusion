Learn more on my website: https://fusion.ifree.page
Link to repository: https://github.com/methodicaldebugger/fusion.git

!!!build bridges between communities!!!

Hello everyone, I would like to build fusion(a brand new programing language). For more details continue reading.

Fusion is an open-source programming language and compiler project with a long-term goal of making software ecosystems interoperate through one language, one toolchain, and one developer experience. The main idea here is knowledge interoperability. "Can a programmer's knowledge transfer across ecosystems?" "Fusion aims to do for programming ecosystems what SQL did for databases: provide a common interface across many underlying implementations."

If you suffer from fragmentation, fusion is for you! This includes: Companies that already use many languages, Library maintainers, Cloud providers, Enterprise software companies, Universities and beginners who learn programing.

Fusion has a easy to learn python-inspired synthax, a rust style debugger, TO THE PROGRAMER IT FEELS LIKE ONE LANGUAGE. A beginner learns Fusion. An expert can access advanced ecosystems when they need them.

A beginner will install fusion(including the fusion toolchain and rust-style debugger + package manager(like cargo))
This allows the user to build: CLI applications, backend services, developer tools, desktop applications, data processing,
scientific computing, games and graphics, cloud applications, distributed systems.

The user could use the package manager(or manualy) download and install plugins. There are 2 types of plugins.
The first type makes fusion more pleasant to use: cloud builds, UI framework, repl + jupyter, AOT + JIT, SQLite support, actors system.
The second type extends fusions reach into existing ecosystems. Ideally fusion would embeed foreign source code, that way it does not need to be rewritten. C plugin could compile C source code, provide little overhead, this way C files do not have to be revritten into fusion. The same goes for other plugins C++, rust, nim, zig, swift, golang, dart, C#, java, Kotlin/native and so on.

Foreign code must live inside explicit foreign-language boundaries(foreign files).



ChatGPT can distinguish between a foreign programing language that has:

easy integration
possible but difficult
possible with a bridge/adapter
possible only for certain libraries
technically possible but not worth doing
effectively impractical


The developer shouldn't have to think:
“Oh no, this is a Python library, therefore I need a completely different API.”
You can't make every arbitrary library magically interoperable so fusion needs adapters!

You may use libraries in other languages, not mentioned above, with a wrapper, binding, ABI boundary, generated bindings, RPC bridge, WebAssembly module, etc.

There are several levels of “foreign library”:
Level 1 — Native ABI. Excellent interoperability. Very little overhead.
Level 2 — Managed runtime. Very powerful, but requires runtime integration.
Level 3 — Embedded runtime. More overhead, but potentially extremely useful.
Level 4 — RPC/process bridge. The foreign library runs independently. This can be surprisingly powerful because the boundary is clean.
Level 5 — WebAssembly. This could provide a very portable integration mechanism.

A possible compiler feature, fusion could even suggest it.

Example:
Warning: Function simulate() consumes 68% of CPU.
Suggestion: Move to Rust for ~8x speedup.

Imagine clicking:
Convert to Rust -> The IDE generates: -> pub fn simulate(...) {    ...     }
and creates the wrapper automatically.

Fusion could say:
Options:
[Optimize Fusion]
[Move to Rust]
[Move to GPU]
[Parallelize]

That is futuristic!



Developers will ask: "I'll build it in Fusion. Which ecosystem is the best implementation for this specific capability?"
When to switch to another languag within fusion?
Only when you need capabilities that Fusion intentionally doesn't try to optimize for. These are almost always libraries, not entire applications.
If you can't build it in fusion, build it in another ecosystem.



Evergreen Technologies(me basicaly)
Evergreen Technologies provides professional services that support organizations building software at scale:
hosted cloud builds, enterprise collaboration features, managed package registries, commercial IDE features, long-term support subscriptions.
These services are commercial, while the core Fusion language and ecosystem remain open.



WE WILL BUILD INTERPRETER-FIRST INTERNALLY, BUT DESIGN COMPILER-FIRST ARCHITECTURALLY!


I would build a very small Fusion language that can call C libraries first, while deliberately designing the compiler so that self-hosting becomes easy later. The reason is strategic: your differentiator isn't "Fusion is a programming language." It's "Fusion lets you use existing software ecosystems without making the programmer learn each ecosystem's language/toolchain." Fusion needs to access the ecosystems of other languages, to connect them, foreign languages and libraries are a valuable asset fusion cannot live without.

So you need to prove that claim early. If Fusion 1.0 can make C libraries feel like ordinary Fusion libraries, I've demonstrated the central idea.
Fusion should be able to integrate or embeed a foreign language NOT inside a .fusion file but in run/compile a separate file(like .c). Foreign languages are project-level dependencies, not syntax-level extensions. If that experience genuinely feels like “I installed Fusion, imported a C library, and never had to think about the C toolchain”, you've demonstrated the central idea behind Fusion.


Fusion will create/have folders as follows:

fusion/
├── compiler/
├── runtime/
├── toolchain/
├── interoperability/
│   ├── native_abi/
│   ├── managed_runtime/
│   ├── embedded_runtime/
│   ├── process_rpc/
│   └── wasm/
├── languages/
│   ├── c/
│   ├── cpp/
│   ├── csharp/
│   ├── golang/
│   ├── swift/
│   ├── zig/
│   ├── swift/
│   ├── rust/
│   ├── nim/
│   ├── dart/
│   └── java/
├── platform/
│   ├── jit+aot/
│   ├── repl+jupyter
│   ├── sqlite support/
│   ├── actors system/
│   ├── ui framework/
│   └── cloud builds/
└── docs/


The user will be able to drop new source_language_support and library_integrations files as they are created by the fusion community.

Fusion should not contain all integrations.
There will be a downloadable adapter/plugin system for every new language.
That way the community can build integrations without modifying the Fusion compiler every time.
Fusion's package manager should ideally make foreign dependencies feel like Fusion dependencies.

Fusion becomes the application-level language while other ecosystems become implementation-level resources/assets.
A Fusion developer shouldn't necessarily ask: “Which language should I use?”
They could ask: “Which implementation/ecosystem is best for this particular capability?”
Fusion becomes the application-level language while other ecosystems become implementation-level resources/assets. Fusion itself must be a good language and Fusion's interoperability makes it useful. A beginner learns fusion, but masters gain capabilities originating from many ecosystems.

If Fusion succeeds, it might look like: “People stopped caring which language their dependency was written in.”
Tutorials and knowledge in many/all programing languages will become useful to a fusion developer.                       

To a developer, he chooses which plugins he will download(if any) and then writes the program in fusion(+ other languages with plugins). This means fusion can call many foreign libraries and embeed other languages via plugins. For example the C plugin will enable the developer to write and compile a C file and of course a SEPARATE fusion file.


Fusion Basic/vanilla has only:

Fusion Programming Language
Syntax, parser, type checker, interpreter/compiler, runtime and standard library

5 Layers of Interoperability
Integration with other languages, runtimes, libraries and native code

DEVELOPMENT TOOLING

Foundry
Package manager for dependencies, plugins, builds, tests, compatibility and publishing.

Toolchain Orchestrator
Coordinates tools, workflows, execution, task scheduling and results.

Static Analyser
Detects code issues, inefficient patterns and potential performance bottlenecks.

Documentation
Language reference, tutorials, standard library guides and tooling documentation.

Everything else is not a part of the basic install, but additionally installable plugins.