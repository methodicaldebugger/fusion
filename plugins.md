Send me files you have made by visiting the link: https://fusion.ifree.page

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

You don't need to rewrite your 20-million-line Java/C++/C# codebase/files. Keep it. Write new software in Fusion and consume the existing systems.

A tutorial written for Python, Rust, Java, C#, C++, or another ecosystem should become transferable knowledge.


-----------------------------------------


C should be Fusion's first serious interoperability target.
Installing it should be incredibly easy, like it's a fusion dependency

You'd need:
ABI support

Fusion needs to understand things such as:

calling conventions
symbol names
shared libraries
static libraries
structs
unions
enums
primitive types
pointers
function pointers
callbacks
alignment
platform-specific ABI differences

A binding generator and Memory boundary



-----------------------------------------


C# is the second serious interoperability target.
C# isn't primarily an ABI problem.

It's a .NET runtime problem.

You could have:
Fusion -> .NET runtime -> C# assembly

or generate C# wrappers:
Fusion -> generated C# adapter -> .NET

Fusion would need to understand things such as:
assemblies
NuGet packages
CLR types
delegates
exceptions
async Tasks
generics
GC interaction
reflection
marshalling

Embedding or interoperating with the CLR is a major engineering project.
But conceptually it's very achievable.








-----------------------------




At this point you have the tools needed to build most of the worlds software:

Use Fusion for the majority of application-level logic:
business logic
application architecture
networking
web servers
APIs
CLI tools
orchestration
data processing
UI logic
application state
concurrency
database code
etc.
C


Use C when you need:
operating-system interfaces
hardware access
embedded systems
drivers
low-level systems programming
extremely predictable low-level behavior
native libraries
platform APIs
existing C ecosystem
C#


Use C# when the .NET ecosystem is the best tool:
.NET libraries
enterprise software
Windows APIs/ecosystem
existing C# libraries
application frameworks
tooling available through NuGet
other .NET languages/components



Now we continue with other languages.
-----------------------------




C++ interoperability is slightly harder than C

I'd not attempt to expose every C++ feature directly.
Stable C++ APIs → generated Fusion bindings.

So I'd use:

C++ library -> C-compatible/stable wrapper -> Fusion binding

or:

C++ headers -> C++ binding generator -> Fusion wrapper




-----------------------------------------




Rust interoperability is slightly harder than C

Rust library -> Fusion-compatible interface -> generated wrapper -> Fusion

A Rust library could explicitly expose an interoperability layer and Fusion tooling generates the necessary bridge.

Rust-specific concepts such as:

ownership
borrowing
lifetimes
traits
async
generics

would need to be translated into Fusion-compatible concepts, rather than exposing Rust's type system directly.





--------------------------------





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






--------------------------------






Nim is actually an interesting target because it can compile to native code and has C interoperability.

You could potentially use:

Nim -> C ABI -> Fusion

for a large portion of its ecosystem.

A richer integration could eventually understand Nim-specific constructs, but this probably wouldn't be an early priority.







------------------------------







Go has a complication: its runtime and garbage collector.

You can use mechanisms such as C-compatible exports and cgo, but arbitrary Go objects cannot simply be treated like C objects.

You'd need to define:

Fusion ↔ Go boundary

with clear rules for:

Go GC
goroutines
channels
callbacks
strings
slices
interfaces
errors
runtime lifetime

A generated Go adapter would likely be the cleanest long-term approach.





--------------------------




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






---------------------------





This one is interesting because Kotlin has multiple execution environments.

Kotlin/JVM isn't the same problem as Kotlin/Native.

For Kotlin/Native, you'd potentially leverage its native interoperability mechanisms.

Conceptually:

Kotlin/Native -> native library -> ABI -> Fusion

But you still have to handle Kotlin's:

runtime
GC
object model
generics
exceptions
concurrency model

So again, a generated boundary is likely preferable.





---------------------------------






Java is similar to C# but with the JVM.

Potential architecture: Fusion -> JVM bridge -> Java library

You'd need to deal with:

JVM startup
class loading
JAR files
Java types
generics
exceptions
threads
GC
JNI
JVM lifecycle

You might eventually have:
fusion add maven:org.example/library

and Fusion tooling handles the JVM dependency automatically.
That would be extremely powerful, but it is a substantial runtime integration.