Fusion interoperability model.

This crate defines the language/toolchain-neutral vocabulary for crossing the Fusion/foreign boundary. It deliberately contains no compiler process management. Foundry resolves *what* a project needs; 

the toolchain orchestrator decides *how/with which tools* to build it.

There are 5 layers of interoperability:
1. Native ABI: the foreign code is compiled into a static or shared library and linked into the Fusion binary. The foreign code is called directly from Fusion code, and vice versa, using the platform's native ABI.
2. Managed runtime: the foreign code is compiled into a library that is loaded into the Fusion process, and called via a managed runtime (e.g. JVM, CLR, etc.). The foreign code is called from Fusion code via the managed runtime's interop facilities, and vice versa.
3. Embedded runtime: the foreign code is compiled into a library that is loaded into the Fusion process, and called via an embedded runtime.
4. Process: the foreign code is compiled into a separate executable that is launched by the Fusion process. The foreign code is called from Fusion code via inter-process communication (IPC), and vice versa.
5. WebAssembly: the foreign code is compiled into a WebAssembly module that is loaded into the Fusion process, and called via the WebAssembly runtime.