# SQLite Support

This plugin provides an integration point for SQLite database support.

SQLite is useful for:

- local applications
- embedded databases
- CLI tools
- testing
- development
- small services
- desktop applications
- mobile applications

## Responsibilities

A future implementation may provide:

- SQLite library discovery
- connection management
- prepared statements
- parameter binding
- row decoding
- transactions
- error translation
- migrations
- resource cleanup

## Memory boundary

Database handles and returned data need explicit ownership rules.

For example:

    Fusion
       |
       v
    SQLite adapter
       |
       v
    sqlite connection

The adapter should ensure that native database resources are released
deterministically.

Fusion's `defer` mechanism may be useful for deterministic cleanup.

## Initial milestone

Implement a narrow prepared-statement API and integration tests using a
temporary database.

## Status

Scaffold only.