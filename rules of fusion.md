
Language philosophy

Fusion is intended to be:
easy to learn, strongly typed, statically typed, type-inferred, expressive, practical
interoperable, native, garbage-collected, suitable for both beginners and experienced developers.

It will have both indentation and brackets:
main: #<-- this means indentation will be used  
main{ //<-- this means brackets will be used
}

It will have a modern rust debugger, rust iterators(for loops), rust match statements.
No classes or pointers, instead use structs and traits.


The language takes inspiration from several ecosystems without attempting to reproduce them exactly.
For example:
Python → approachable syntax and indentation
Rust → algebraic data types, pattern matching, iterators, diagnostics
Dart → managed runtime and potential JIT/AOT architecture
LLVM → compiler infrastructure
SQL → the idea of a common abstraction over different implementations








Fusion is its own language.
Syntax goals
Fusion's official style uses indentation.
Braces may also be supported for developers who prefer C-style syntax, but a file should consistently use one style.

Indentation
main:
    for 0..10:
        print("hello")
Braces
main {
    while x < 10 {
        print(x)
        x += 1
    }
}

The parser determines the chosen style at the beginning of the file and maintains that choice consistently.
The goal is to minimize boilerplate while allowing developers to use a familiar structural style.

Comments may be written like in C and python with:
<//> <- single line comment
<#> <- single line comment
</*>
multiline comment
<*/>
<""">
multiline comment
<""">

Type system: Fusion is intended to be statically and strongly typed with type inference.
For example:
x = true
name = "Fusion"
count = 42
const version = 1


Explicit types can also be written:
int x = 42
float y
string name = "Fusion"
char initial = 'F'
int[] numbers = [1, 2, 3]


Variables are mutable by default.
Use const for values that should not change.

Instead of a array, vector, list, linked list, slice, dynamic array. We only have a growable array!!
A Fusion programmer doesn't need to learn five different collection types just to store a sequence of things.

int[] numbers = [1, 2, 3]
string[] names = [
    "Alice",
    "Bob",
    "Charlie"
]
print(numbers[0])
numbers[2] = 42
x = numbers[i]
length: numbers.length

                                                                                                               

Built-in methods: numbers.push(5), numbers.pop(), numbers.clear(), numbers.contains(42), numbers.sort(), numbers.reverse(), length: numbers.length
Printing variables: print(x), print("Hello"), print("{name} is {age} years old.")
debug(person) // debug always shows the full structural representation.
Fusion is statically typed, it can automatically call the appropriate formatting code


Fusion is like zig it lets you write conceptually: 
file = open("data.txt") 
defer file.close() 
process(file) 
The meaning is: When this scope exits, execute file.close(). You don't have to remember to put: file.close() at every possible exit point.


Fusion will have:
GC → automatically manages memory
defer → deterministically releases resources
scopes → let programmers control when deferred cleanup happens

That's much simpler than importing C++'s entire RAII/ownership system.
When file leaves scope, its destructor runs. This makes resource management deterministic.
Fusion takeaway: Even with GC, deterministic resource cleanup is extremely valuable for things like files, sockets, GPU resources, database connections, and foreign handles.


Make Option and Result foundational:
Option instead of null "Option<int>" This eliminates a huge class of null-pointer bugs.
Result for errors "Result<T, E>" Instead of throwing exceptions everywhere.


Fusion intends to provide Rust-style algebraic data types and pattern matching.
The goals include: exhaustive matching, nested patterns, destructuring
conditional patterns, safe reference matching, compiler optimization.

Example:
match value:
    Some(x):
        print(x)
    None:
        print("Nothing")

Pattern matching is intended to reduce complex chains of conditional logic.

Iterators
Fusion intends to provide a deliberately small collection of useful iterators.
Planned operations include:
map, filter, reduce, fold, collect, find, any, all, count, take, skip, zip, flatten.

The goal is to provide expressive collection processing while allowing the compiler to optimize common iterator patterns.







Memory management
Fusion is planned to use automatic memory management.
The initial direction is a garbage-collected runtime, rather than:
manual memory management
reference counting
ownership
borrow checking
lifetime annotations

This is an intentional design decision.
Fusion is not trying to reproduce Rust's ownership model.
However, this also means Fusion will not attempt to replace every use case for languages such as C++ or Rust.


Low-level systems programming may still benefit from languages designed specifically for:
kernels
firmware
embedded systems
operating-system internals
compilers
specialized engines


Fusion's goal is interoperability, not universal replacement.
Functions
Functions return values explicitly.
fn add(a, b):
    return a + b
Functions that do not return a value do not need a return type.


Structs and traits
Fusion plans to support:
structs
traits
generics
properties
modules
algebraic data types

Fusion intentionally does not plan to use traditional class inheritance.
The goal is to favor composition and explicit relationships.


What Fusion intentionally does not try to be
Fusion is not intended to replace every programming language.

The project does not initially aim to provide:

ownership/borrow checking
lifetime annotations
complex macros
header files
preprocessor directives
class inheritance
traditional classes
arbitrary loop constructs
manual pointer-based programming as a primary abstraction

Fusion is intentionally making different tradeoffs.


Concurrency
Fusion plans to support:
async and await.
The exact concurrency and runtime model remains an area of design and experimentation.


Compiler architecture
The long-term compiler architecture is expected to resemble:
Fusion source
      │
      ▼
Fusion compiler
      │
      ▼
Fusion IR
      │
      ▼
LLVM
      │
      ▼
Machine code

LLVM provides mature compiler infrastructure for optimization and native code generation.
Building an entirely independent optimizer and code generator would dramatically increase the scope of the project.
Fusion can instead focus its engineering effort on:
language semantics, type checking, intermediate representation
interoperability, runtime design, developer tooling, ecosystem integration
Interpreter-first, compiler-first architecture

Fusion may initially use an interpreter because it provides a fast development cycle.







However:
Build the interpreter first, but design the architecture around the compiler.
This allows language experimentation without locking the project into an interpreter-only execution model.

JIT and AOT
A long-term goal is to support both:
JIT. Just-in-Time compilation can optimize code based on how it actually runs.
AOT. Ahead-of-Time compilation can produce native executables with:

repl + jupyiter

And of course language interoperability, this means the user should be able to
install the C plugin and write C code, install the rust plugin and write rust code...
as well as use foreign libraries without any additional installation.