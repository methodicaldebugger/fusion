A layered interoperability architecture is required:

One for Native/foreign language integration:
It lets developers write, compile, debug, and maintain foreign-language source code.
Such as C, C++, rust, etc.

Another for Foreign library integration:
The developer doesn't necessarily write Python for example.
The Fusion developer sees only the Fusion interface.

They don't necessarily need:
python, pip, virtualenv, conda.
installed separately.

Fusion's package/runtime manager could manage that environment.


For a TypeScript library: the developer shouldn't necessarily have to install Node.js just to consume one package. 
Fusion could package the required runtime and dependency graph.



There are several levels of “foreign library”:
Level 1 — Native ABI. Excellent interoperability. Very little overhead.
Level 2 — Managed runtime. Very powerful, but requires runtime integration.
Level 3 — Embedded runtime. More overhead, but potentially extremely useful.
Level 4 — RPC/process bridge. The foreign library runs independently. This can be surprisingly powerful because the boundary is clean.
Level 5 — WebAssembly. This could provide a very portable integration mechanism.


Fusion should hide these differences!
The developer shouldn't have to think:
“Oh no, this is a Python library, therefore I need a completely different API.”
You can't make every arbitrary library magically interoperable so fusion needs adapters!