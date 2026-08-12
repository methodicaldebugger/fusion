Don't underestimate the community:
Rust experts should guide Rust integration. Dart experts should guide Dart integration. and so on.

Would maintenance be a nightmare?
Only if Fusion itself tries to maintain every ecosystem. A modular integration architecture and community-owned bridges can prevent that.

Would it be too large?
Use a minimal core + downloadable toolchains + optional integrations + cloud builds. Local/cloud idea is very strong here. There should be a small default Fusion installation. Small enough to install normally. Then download plugins for other languages(to write them, not to use foreign libraries).

There is a very important distinction:
“One language that can access many ecosystems” is plausible.
“One language that transparently consumes essentially every library from every major language” is extraordinarily difficult.

The architecture needs to be more disciplined!
Don't build everything at once, fusion 1.0(just a new programing language) is very possible
Fusion 2.0(Package manager, IDE + debugger + LSP, JIT + AOT, Excellent standard library, interoperability with other languages) is much harder but possible
Fusion 3.0(Interoperability with 10+ languages and maintaining them) very difficult but possible