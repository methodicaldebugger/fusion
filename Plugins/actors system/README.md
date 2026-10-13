# Actors System

This plugin explores an actor-based concurrency model for Fusion.

Actors can provide:

- isolated mutable state
- message passing
- asynchronous coordination
- supervision
- fault isolation
- independent execution contexts

## Relationship with async/await

Fusion already intends to support async/await.

The actor system should complement async/await rather than replace it.

A possible model is:

    Fusion async tasks
           |
           v
       Actor runtime
           |
      +----+----+
      |         |
    Actor     Actor
      |         |
    mailbox   mailbox

## Concepts

The plugin should eventually define:

- actor identity
- mailboxes
- messages
- message ownership
- scheduling
- supervision
- shutdown
- failure propagation
- backpressure
- actor lifecycle

## Deterministic testing

A deterministic test scheduler would be useful before implementing a fully
concurrent production scheduler.

This allows actor behavior to be tested without relying entirely on timing.

## Initial milestone

Define actor/message semantics and implement a deterministic test runtime.

## Status

Scaffold only.