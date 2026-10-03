# Java Interoperability Plugin

Java interoperability is primarily a JVM integration problem.

A possible architecture is:

    Fusion
       |
       v
    JVM bridge
       |
       v
    Java library

## JVM responsibilities

Fusion tooling eventually needs to understand:

- JVM startup
- JVM lifecycle
- class loading
- JAR files
- Maven dependencies
- Java types
- generics
- exceptions
- threads
- garbage collection
- JNI
- object references

## Maven

A future Foundry workflow could eventually support:

    fusion add maven:org.example/library

Foundry could resolve the Maven dependency graph and configure the JVM
integration automatically.

## Runtime boundary

The JVM must have an explicit lifecycle.

The integration should define:

- startup
- class loaders
- object references
- thread attachment
- exception translation
- asynchronous operations
- shutdown

## Initial milestone

Connect a tiny Java library through a JVM bridge.

Do not attempt complete Java interoperability initially.

## Status

Scaffold only.