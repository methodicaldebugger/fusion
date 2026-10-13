# JIT + AOT

This plugin provides a future integration point for Fusion execution backends.
It covers two major execution strategies:

## JIT

Just-in-time compilation can compile Fusion code during execution.

Potential uses include:

- REPL execution
- development
- dynamic workloads
- rapid iteration
- runtime specialization

JIT compilation is especially useful when a runtime can observe how a program actually behaves.
For example, the Java Virtual Machine can detect that a function is called millions of times and optimize it.

## AOT

Ahead-of-time compilation produces deployable artifacts before execution.

Potential uses include:

- production applications
- command-line applications
- embedded systems
- native deployment
- predictable startup

## Enables

AOT (Ahead-of-Time) compilation means compiling code into machine code before the program runs, rather than compiling it during execution (JIT) or interpreting it at runtime. It enables a programming language to offer several important capabilities.

Fast startup and predictable execution, native machine-code performance, the compiler can optimize code before execution.
Easier deployment, a compiler can produce a native executable where users don't need anything installed.
Many errors can be detected before execution. This is especially powerful when AOT compilation is combined with a statically typed language, as in fusion, Rust, C++, or Go. 
Optimization for a specific target. The compiler can generate code for a particular CPU architecture and operating system.

The trade-offs include:
- JIT: potential for excellent peak performance, but compilation overhead and warm-up.
- AOT: native code is ready before execution, often giving faster startup and simpler deployment.
- Interpretation: flexible execution and convenient development, but potentially more runtime overhead.

## Architecture

A future architecture could look like:

    Fusion source
         |
         v
      Fusion IR
         |
       +---+
       |   |
       v   v
      JIT AOT
       |   |
       +---+
         |
         v
      Runtime

The backend should integrate with the compiler architecture rather than
creating a completely separate compiler.

## Responsibilities

The plugin may eventually handle:

- backend discovery
- target selection
- code generation
- runtime selection
- compilation caches
- diagnostics
- artifact metadata
- debug information
- optimization configuration

## Status

Scaffold only.