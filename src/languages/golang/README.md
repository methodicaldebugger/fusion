# Go Interoperability Plugin

Go has an important complication: its runtime and garbage collector.

Arbitrary Go objects should not be treated as though they were ordinary C
structures.

Go can expose C-compatible entry points, but the runtime boundary still needs
to be respected.

## Boundary

A likely long-term architecture is:

    Fusion
       |
       v
    Generated Go adapter
       |
       v
    Go runtime

The integration needs explicit rules for:

- Go GC
- handles
- goroutines
- channels
- callbacks
- strings
- slices
- interfaces
- errors
- runtime lifetime

## cgo

cgo and C-compatible exports can be useful for narrow interfaces.

They should not be interpreted as meaning that arbitrary Go objects have
become ordinary native ABI objects.

## Initial milestone

Create a small Go adapter using opaque handles and explicit lifecycle
functions.

## Status

Scaffold only.