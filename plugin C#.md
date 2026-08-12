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