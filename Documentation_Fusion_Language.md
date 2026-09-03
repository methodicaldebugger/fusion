# Official Technical Reference & Language Guide: Fusion 1.0

---

## 1. Introduction & Philosophy

Fusion is a strongly typed, statically compiled, type-inferred programming language built to maximize expressiveness and ergonomics without sacrificing performance or maintainability. Designed to serve both beginners and seasoned systems/application engineers, Fusion blends the syntax usability of Python with the functional power and safety guarantees of Rust, backed by a managed runtime inspired by Dart.

### Core Principles
* **Approachable Syntax:** Clean, flexible formatting options inspired by Python with low boilerplate.
* **Safety Without Overhead:** Standardized algebraic data types (`Option`, `Result`) and pattern matching prevent null-pointer exceptions and state hazards.
* **Simplified Core:** A unified growable array structure replaces complex, fragmented collection hierarchies.
* **Composition Over Inheritance:** Pure struct and trait composition models eliminate class hierarchy side effects.
* **Extensible Ecosystem Architecture:** Designed from day one to support JIT/AOT execution, rich editor diagnostics, and interoperability layers for ecosystem growth.

### Influences & Tradeoffs

```text
┌────────────────────────────────────────────────────────────────────────┐
│                               INFLUENCES                               │
├───────────────┬─────────────────┬───────────────────┬──────────────────┤
│    Python     │      Rust       │       Dart        │       SQL        │
│ Approaching   │ ADTs, Pattern   │  Managed Runtime, │ Common Data      │
│ Syntax &      │ Matching,       │  Unified JIT/AOT  │ Abstraction      │
│ Indentation   │ Iterators       │   Architecture    │ Models           │
└───────────────┴─────────────────┴───────────────────┴──────────────────┘
```

#### What Fusion Explicitly Excludes
To preserve simplicity and keep compile times low, Fusion intentionally omits:
* Ownership annotations, borrow checkers, and manual lifetime parameters
* Preprocessor directives, header files, and unsafe macro systems
* Traditional class inheritance and object-oriented dynamic dispatch tables
* Raw pointers as a primary user-level abstraction
* Complex, overlapping loop keywords and multiple concrete sequence types

---

## 2. Syntax & Lexical Structure

### Dual Structural Styles
Fusion permits two structural code formatting styles. A single file **must** consistently adhere to one style.

#### 1. Indentation-Based (Python Style)
Indentation blocks are marked by a trailing colon `:` at the end of the signature or control line.

```fusion
fn main():
    x = 42
    if x > 10:
        print("Value is large")
```

#### 2. Bracket-Based (C Style)
Explicit scoping blocks are delimited using standard curly braces `{}`.

```fusion
fn main() {
    x = 42;
    if (x > 10) {
        print("Value is large");
    }
}
```

### Comment Syntax
Fusion utilizes explicit tag-delimited comment structures:

```fusion
<//> This is a single-line comment
<#> This is also a single-line comment

</*>
    This is a multi-line comment block.
    It can span arbitrary lines.
<*/>
```

---

## 3. Type System & Variable Declarations

Fusion combines static type safety with type inference. All types are checked at compile time.

### Mutability & Binding
* **Mutable by Default:** Standard bindings declared with implicit or explicit types can be reassigned.
* **Constants:** Declared using the `const` keyword. Immutable once initialized.

```fusion
<//> Type Inference
x = true               <//> Inferred as bool
name = "Fusion"        <//> Inferred as string
count = 42             <//> Inferred as num

<//> Constant Binding
const version = 1

<//> Explicit Type Declarations
num x = 42
float y = 3.4
bool z = true
string name = "Fusion"
num[] numbers = [1, 2, 3]
```

---

## 4. Primitive Types & Primary Collections

### The Unified Sequence: Growable Arrays
Instead of dividing sequence operations across slices, dynamic vectors, static arrays, and linked lists, Fusion provides a single, uniform type: **The Growable Array** (`T[]`).

```fusion
num[] numbers = [1, 2, 3]
string[] names = [
    "Alice",
    "Bob",
    "Charlie"
]

<//> Element Access and Mutation
print(numbers[0])
numbers[2] = 42

<//> Array Field Properties
num len = numbers.length
```

#### Standard Array Methods

| Method | Description | Signature / Usage |
| :--- | :--- | :--- |
| `.push(val)` | Appends an element to the end | `numbers.push(5)` |
| `.pop()` | Removes and returns the last element | `numbers.pop()` |
| `.clear()` | Removes all elements | `numbers.clear()` |
| `.contains(val)`| Returns `bool` if value is present | `numbers.contains(42)` |
| `.sort()` | In-place ordering of items | `numbers.sort()` |
| `.reverse()` | Reverses array element order | `numbers.reverse()` |
| `.length` | Evaluates array size | `numbers.length` |

---

## 5. Built-in I/O & Diagnostics

Fusion provides native standard printing alongside structural runtime inspection:

```fusion
string name = "Fusion"
num age = 1

<//> Standard Output & Interpolation
print(x)
print("Hello")
print("{name} is {age} years old.")

<//> Deep Structural Debugging
debug(person) <//> Emits full structural breakdown (fields, types, memory layout)
```

---

## 6. Resource & Memory Management

Fusion combines an automatic Garbage Collector (GC) with explicit block-scoped resource determinism (`defer`).

```fusion
fn process_file(path):
    file = open(path)
    defer file.close() <//> Guarantees file.close() executes on scope departure
    
    <//> Work performed on file
    read_data(file)
    <//> file.close() automatically runs here upon function/scope return
```

### Key Operational Characteristics
1. **Garbage Collection (GC):** Handles general heap allocations, freeing developers from manual deallocation and borrow-checking lifetime annotations.
2. **`defer` Statements:** Enforces deterministic resource cleanup (file handles, network sockets, database locks) upon exiting the surrounding lexical scope.
3. **Lexical Scopes:** Provide precise bounds for when deferred tasks execute.

---

## 7. Error Handling & Safety (`Option` & `Result`)

Fusion avoids `null` pointers and unhandled runtime exceptions by incorporating foundational algebraic safety types.

```text
                  ┌──────────────────────────────┐
                  │      SAFETY TYPE MODEL       │
                  └──────────────┬───────────────┘
                                 │
           ┌─────────────────────┴─────────────────────┐
           ▼                                           ▼
┌──────────────────────┐                    ┌──────────────────────┐
│      Option<T>       │                    │     Result<T, E>     │
├──────────────────────┤                    ├──────────────────────┤
│  • Some(T)           │                    │  • Ok(T)             │
│  • None              │                    │  • Err(E)            │
│  (Replaces Null)     │                    │  (Replaces Thrown    │
│                      │                    │   Exceptions)        │
└──────────────────────┘                    └──────────────────────┘
```

### Option Type
Represents optional existence without null safety hazards:

```fusion
Option<num> find_index(num[] list, num target):
    <//> Implementation logic
    return Some(index)
    <//> Or return None
```

### Result Type
Explicitly isolates recoverable runtime operational errors:

```fusion
Result<File, String> open_config(string path):
    if exists(path):
        return Ok(file_handle)
    return Err("File not found")
```

---

## 8. Pattern Matching & ADTs

Pattern matching in Fusion provides exhaustive branching, nested destructuring, and conditional verification.

```fusion
match value:
    Some(x):
        print("Value found: {x}")
    None:
        print("Nothing found")

match result:
    Ok(data):
        process(data)
    Err(msg):
        print("Error encountered: {msg}")
```

### Advanced Pattern Capabilities
* **Exhaustiveness Checking:** The compiler validates that all potential variant states are addressed.
* **Nested Patterns:** Match inner structures or wrapped options directly (`Some(Some(val))`).
* **Conditional Guards:** Append extra logical assertions directly onto matching branches.

---

## 9. Iterators & Functional Sequences

Fusion provides a optimized pipeline of iterator operations that avoid unnecessary intermediate array allocations.

```fusion
num[] items = [1, 2, 3, 4, 5]

num total = items.iter()
    .filter(fn(x): return x % 2 == 0)
    .map(fn(x): return x * 2)
    .fold(0, fn(acc, x): return acc + x)
```

### Standard Iterator API Surface
* **Transformation:** `map`, `flatten`, `zip`
* **Filtering & Selection:** `filter`, `take`, `skip`, `find`
* **Aggregation & Evaluation:** `reduce`, `fold`, `collect`, `count`, `any`, `all`

---

## 10. Type Abstraction: Structs & Traits

Fusion rejects classical class hierarchies in favor of explicit structure definitions and contract traits.

```fusion
<//> Struct Definition
struct Point:
    num x
    num y

<//> Trait Contract
trait Printable:
    fn to_string(): string

<//> Implementing Traits on Structs
impl Printable for Point:
    fn to_string():
        return "Point({this.x}, {this.y})"
```

### Object Model Rules
* **No Class Inheritance:** Code reuse relies on struct embedding and trait composition.
* **Generics:** Structs, traits, and functions support parametric polymorphism (`struct Stack<T>`).
* **Modules:** Clean encapsulation across logical project domains.

---

## 11. Concurrency Model

Fusion standardizes asynchronous execution using explicit `async` and `await` markers.

```fusion
async fn fetch_data(string url):
    response = await http_get(url)
    return response.body
```

### Concurrency Runtime
* Standard asynchronous task scheduling via non-blocking event loops.
* Extensible architecture tailored to support multi-threaded task distribution and actor-based message passing.

---

## 12. Compiler Pipeline & Tooling Architecture

The Fusion compiler infrastructure is constructed around a multi-stage Intermediate Representation (IR) design to support high-performance diagnostics, fast interpreted execution, and native binary generation.

### Compilation Pipeline Map

```text
                  Fusion Source Code
                           │
                           ▼
                         Lexer
                           │
                     Token + Span
                           │
                           ▼
                         Parser
                           │
                           ▼
                          AST
                           │
              ┌────────────┴────────────┐
              │                         │
        Source Spans               Syntax Only
              │                         │
              └────────────┬────────────┘
                           ▼
                    Name Resolution
                           │
                           ▼
                       Typed HIR
                           │
                     Type Checking
                           │
                           ▼
                          MIR
                           │
        ┌──────────────────┼──────────────────┐
        ▼                  ▼                  ▼
   Interpreter            JIT                AOT
        │                  │                  │
        └──────────────────┼──────────────────┘
                           ▼
                     Fusion Runtime
```

### Stage Definitions

1. **Lexer & Tokens:** Transforms plain source text into token streams decorated with precise byte spans (`Span`).
2. **Parser & AST:** Generates an Abstract Syntax Tree maintaining strict links to source locations for contextual error diagnostics.
3. **Name Resolution:** Resolves symbol definitions, scope access rules, and module boundaries.
4. **Typed High-Level IR (HIR):** Executes type inference algorithms, enforces constraint checking, and verifies exhaustiveness.
5. **Mid-Level IR (MIR):** Produces a control-flow graph optimized for target-independent passes, variable liveness tracking, and iterator fusion.
6. **Execution Backends:**
   * **Interpreter:** Executes directly from MIR for instant incremental feedback loops.
   * **JIT Engine:** Compiles hot MIR paths into machine code at runtime.
   * **AOT Compiler:** Translates MIR into standalone, optimized native binaries.

---

## 13. System Ecosystem & Interoperability (Fusion 1.0)

Fusion 1.0 provides a extensible architecture designed to let community developers build specialized framework integrations without modifying the core compiler.

```text
                    ┌───────────────────────────┐
                    │     FUSION 1.0 KERNEL     │
                    └─────────────┬─────────────┘
                                  │
      ┌──────────────┬────────────┼────────────┬──────────────┐
      ▼              ▼            ▼            ▼              ▼
┌───────────┐  ┌───────────┐  ┌───────┐  ┌───────────┐  ┌───────────┐
│ 5-Layer   │  │ REPL /    │  │ SQLite│  │ Flutter-  │  │ Actor     │
│ Interop   │  │ Jupyter   │  │ Native│  │ Style UI  │  │ System    │
│ Engine    │  │ Kernel    │  │ Driver│  │ Framework │  │ Runtime   │
└───────────┘  └───────────┘  └───────┘  └───────────┘  └───────────┘
```

### Architected Community Integration Points
1. **5-Layer Interoperability System:** Clean foreign-function interface (FFI) bindings for C, C++, Rust, and host system runtimes.
2. **REPL & Notebook Ecosystem:** Standardized hooks into MIR interpretation for Jupyter kernels and interactive shells.
3. **Actor Concurrency Frameworks:** Core runtime primitive bindings for Elixir/Erlang-style lightweight message-passing processes.
4. **Native UI & Graphics Rendering:** Efficient memory layouts and JIT bridges designed for Flutter-style reactive UI trees.
5. **Embedded Database Abstractions:** Native bindings tailored for zero-copy SQLite data exchange and generalized query abstractions.
