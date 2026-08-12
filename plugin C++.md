C++ interoperability is slightly harder than C

I'd not attempt to expose every C++ feature directly.
Stable C++ APIs → generated Fusion bindings.

So I'd use:

C++ library -> C-compatible/stable wrapper -> Fusion binding

or:

C++ headers -> C++ binding generator -> Fusion wrapper

