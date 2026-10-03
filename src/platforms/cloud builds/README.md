# Cloud Builds

This plugin provides an integration point for remote Fusion builds.

The goal is not simply to "run the compiler on a server".

A cloud build system should understand Fusion's build graph and provide
reproducible remote execution.

## Potential architecture

    Fusion project
         |
         v
       Foundry
         |
         v
    Build graph
         |
         v
    Remote executor
         |
         v
    Build worker
         |
         v
    Artifacts
         |
         v
    Fusion project

## Potential responsibilities

- package build inputs
- identify dependencies
- select workers
- upload source/artifacts
- execute build tasks
- cache build inputs
- stream logs
- collect diagnostics
- retrieve artifacts
- verify artifacts
- support reproducible builds

## Content-addressed caching

A future implementation could use content-addressed inputs and outputs.

For example:

    source hash
        +
    dependency hashes
        +
    toolchain hash
        |
        v
    reproducible build key

This can make remote build caching substantially more reliable.

## Security

Cloud builds introduce an important security boundary.

The system needs to consider:

- untrusted source code
- malicious build scripts
- credentials
- secrets
- worker isolation
- network access
- artifact verification
- source privacy
- dependency provenance

Secrets should never be placed directly into project manifests.

## Local/remote parity

A cloud build should produce the same result as the corresponding local build
whenever the build is declared reproducible.

The build graph should therefore be shared between local and remote execution.

## Initial milestone

First define a local executor interface.

Then build a reproducible local sandbox.

Only after that should remote workers be introduced.

## Status

Scaffold only.