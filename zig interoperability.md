Zig is comparatively friendly.

Zig has excellent C interoperability and is designed for systems programming.

You could potentially use:

Fusion -> C ABI -> Zig

for many libraries.

But Zig also has:

comptime
allocators
slices
error unions
optionals
compile-time execution

So a direct Zig binding generator would eventually need to understand those concepts.

I'd initially prioritize Zig libraries exposing C-compatible interfaces.