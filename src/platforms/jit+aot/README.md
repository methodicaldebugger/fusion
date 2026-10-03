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

## AOT

Ahead-of-time compilation produces deployable artifacts before execution.

Potential uses include:

- production applications
- command-line applications
- embedded systems
- native deployment
- predictable startup

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