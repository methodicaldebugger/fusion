# REPL + Jupyter

This plugin provides interactive Fusion execution.

It has two related but distinct goals.

## REPL

The Fusion REPL should support:

- incremental parsing
- incremental type checking
- evaluation
- state persistence
- history
- diagnostics
- cancellation
- multi-line expressions

For example:

    fusion repl

## Jupyter

A Jupyter kernel would allow Fusion programs to run inside notebooks.

The kernel would need to implement:

- Jupyter messaging
- execution counts
- code execution
- output
- errors
- rich display values
- interruption
- kernel lifecycle

The Jupyter protocol should remain separate from the Fusion evaluator.

Architecture:

    Jupyter
       |
       v
    Fusion kernel adapter
       |
       v
    Fusion evaluator

## Initial milestone

Implement a simple line-oriented Fusion REPL first.

Then build the Jupyter adapter around the same evaluation interface.

## Status

Scaffold only.