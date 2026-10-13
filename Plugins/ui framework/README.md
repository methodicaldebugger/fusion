# UI Framework

This plugin is an integration point for external UI frameworks.

It is intentionally not a UI toolkit itself.

Different platforms have different UI systems, event loops, rendering
backends, resource formats, and application packaging rules.

## Potential responsibilities

The plugin may eventually coordinate:

- UI framework discovery
- native event loops
- rendering backends
- generated bindings
- platform handles
- resources
- fonts
- images
- application packaging
- platform-specific build tools

## Architecture

A possible model is:

    Fusion UI code
          |
          v
    Fusion UI abstraction
          |
          v
      UI adapter
       /     \
      v       v
  Native UI  Web UI

The exact architecture should remain flexible until a concrete framework is
selected.

## Platform support

Desktop, mobile, and web UI should be treated as separate capabilities.

For example:

- Windows
- macOS
- Linux
- iOS
- Android
- Web

should not automatically be assumed to have identical capabilities.

## Initial milestone

Choose one framework and one target platform for a minimal proof of concept.

Document event-loop ownership and native handle lifetimes.

## Status

Scaffold only.