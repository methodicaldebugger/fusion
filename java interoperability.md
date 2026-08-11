Java is similar to C# but with the JVM.

Potential architecture: Fusion -> JVM bridge -> Java library

You'd need to deal with:

JVM startup
class loading
JAR files
Java types
generics
exceptions
threads
GC
JNI
JVM lifecycle

You might eventually have:
fusion add maven:org.example/library

and Fusion tooling handles the JVM dependency automatically.
That would be extremely powerful, but it is a substantial runtime integration.