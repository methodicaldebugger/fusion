# Fusion Platform Plugins

This directory contains optional Foundry plugins for capabilities that extend
Fusion beyond its basic compiler/runtime workflow.

These are deliberately separated from language plugins.

Language plugins answer:

    "How does Fusion interoperate with another programming ecosystem?"

Platform plugins answer:

    "How does Fusion integrate with a particular execution environment,
     development workflow, library, concurrency model, UI system, or
     infrastructure?"

## Plugin philosophy

These capabilities should remain optional.

Fusion Basic should remain sufficient for writing and running ordinary Fusion
programs.

Additional functionality can be installed when a project needs it.

Examples include:

- JIT/AOT execution
- Jupyter
- SQLite
- actor systems
- UI frameworks
- cloud build infrastructure

## Responsibilities

Platform plugins should describe:

- capabilities
- supported targets
- dependencies
- runtime requirements
- generated artifacts
- lifecycle
- configuration
- diagnostics
- compatibility
- security requirements
- reproducibility

They should integrate with Foundry and the toolchain orchestrator rather than
duplicating functionality unnecessarily.

## Contents

- JIT + AOT
- REPL + Jupyter
- SQLite support
- Actors system
- UI framework
- Cloud builds

## Status

These directories are architectural scaffolds.

They should only advertise capabilities once those capabilities are actually
implemented and tested.