Nim is actually an interesting target because it can compile to native code and has C interoperability.

You could potentially use:

Nim -> C ABI -> Fusion

for a large portion of its ecosystem.

A richer integration could eventually understand Nim-specific constructs, but this probably wouldn't be an early priority.