# Swift Interoperability Plugin

Swift should not be treated as merely another C ABI.

Swift has:

- value and reference semantics
- ARC
- generics
- protocols
- closures
- async/await
- Swift runtime behavior
- Objective-C interoperability
- Apple SDK integration

## Initial strategy

Use Swift's C and Objective-C interoperability as the initial boundary.

For example:

    Swift
       |
       v
    C-compatible wrapper
       |
       v
     Fusion

A richer direct Swift binding generator can be developed later.

## Apple platforms

Apple platform integration is a separate major concern.

Fusion will eventually need to understand things such as:

- SDK discovery
- Xcode toolchains
- frameworks
- platform targets
- application bundles
- signing
- resources
- Apple build conventions

That work should not be hidden inside the basic Swift ABI layer.

## Initial milestone

Build a small Swift library exposing a narrow C-compatible interface.

## Status

Scaffold only.