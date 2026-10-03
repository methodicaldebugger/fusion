# Kotlin Interoperability Plugin

Kotlin is particularly interesting because Kotlin/JVM and Kotlin/Native are
different interoperability problems.

They should not be treated as one generic Kotlin target.

## Kotlin/Native

A possible architecture is:

    Kotlin/Native
         |
         v
    Native library
         |
         v
        ABI
         |
         v
       Fusion

Kotlin/Native's native interoperability mechanisms can provide a useful
starting point.

## Kotlin/JVM

Kotlin/JVM is primarily a JVM integration:

    Fusion
       |
       v
    JVM bridge
       |
       v
    Kotlin/JVM

This is conceptually similar to Java interoperability.

## Runtime concerns

Both routes require careful handling of:

- runtime
- garbage collection
- object model
- generics
- exceptions
- concurrency
- callbacks
- generated interfaces

## Initial milestone

Treat Kotlin/JVM and Kotlin/Native as separate plugin capabilities.

Do not claim that a Kotlin library is universally compatible without knowing
which backend produced it.

## Status

Scaffold only.