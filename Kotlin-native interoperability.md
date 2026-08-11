This one is interesting because Kotlin has multiple execution environments.

Kotlin/JVM isn't the same problem as Kotlin/Native.

For Kotlin/Native, you'd potentially leverage its native interoperability mechanisms.

Conceptually:

Kotlin/Native -> native library -> ABI -> Fusion

But you still have to handle Kotlin's:

runtime
GC
object model
generics
exceptions
concurrency model

So again, a generated boundary is likely preferable.