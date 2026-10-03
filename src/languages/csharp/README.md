# C# Interoperability Plugin

C# is primarily a managed-runtime interoperability problem rather than a
simple native ABI problem.

A possible architecture is:

    Fusion
       |
       v
    .NET runtime
       |
       v
    C# assembly

Another possible architecture is:

    Fusion
       |
       v
    Generated C# adapter
       |
       v
    .NET

## Required concepts

The integration eventually needs to understand:

- .NET assemblies
- NuGet packages
- CLR types
- delegates
- exceptions
- async Tasks
- generics
- garbage collection
- reflection
- marshalling
- assembly loading
- runtime lifecycle

## CLR hosting

Embedding or interoperating with the CLR is a major engineering project.

Fusion should therefore avoid pretending that C# interoperability is simply
another native library.

The runtime boundary must explicitly define:

- who starts the CLR
- who owns the CLR
- how assemblies are loaded
- how objects are represented
- how exceptions cross the boundary
- how Tasks map to Fusion async operations
- how GC references are retained
- how callbacks are handled
- how shutdown works

## NuGet

A future Foundry workflow could eventually support something conceptually like:

    fusion add nuget:Some.Package

Foundry could then resolve the package and its dependencies and construct the
appropriate .NET integration graph.

## Initial milestone

Prototype a generated adapter around a very small .NET assembly.

Do not attempt general CLR hosting until the adapter model is understood.

## Status

Scaffold only.