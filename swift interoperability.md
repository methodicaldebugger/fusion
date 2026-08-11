Swift is another ecosystem where you don't want to pretend everything is a C ABI.

You have:

Swift types
ARC
generics
protocols
closures
async/await
Objective-C interoperability
Swift runtime
Apple frameworks

A realistic first approach could leverage Swift ↔ C/Objective-C interoperability.

For example:

Swift -> C-compatible wrapper -> Fusion

Then eventually build richer direct Swift integration.

For Apple platforms, however, Fusion would also need to understand Apple's SDK/build ecosystem.

That's a significant project by itself.