Go has a complication: its runtime and garbage collector.

You can use mechanisms such as C-compatible exports and cgo, but arbitrary Go objects cannot simply be treated like C objects.

You'd need to define:

Fusion ↔ Go boundary

with clear rules for:

Go GC
goroutines
channels
callbacks
strings
slices
interfaces
errors
runtime lifetime

A generated Go adapter would likely be the cleanest long-term approach.