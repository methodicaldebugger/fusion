Contributing to Fusion

Thank you for your interest in Fusion.

Fusion is an ambitious open-source project whose long-term goal is to connect programming ecosystems through one language, one toolchain, and one developer experience.

Build bridges between communities.

There are many ways to contribute, and not all contributions require writing compiler code.

Before contributing

Please read:

README.md
CODE_OF_CONDUCT.md

If you are working on a specific part of Fusion, also check the relevant documentation under docs/.

Fusion is an early-stage project. Some ideas described in the documentation are long-term goals rather than implemented features.

Please distinguish between:

Implemented — currently working
Experimental — being actively tested
Planned — intended future work
Research — requires investigation
Aspirational — long-term vision

Do not assume that a proposed feature already exists.

Ways to contribute
Code

You can contribute to:

parser development
compiler development
type checking
intermediate representation
interpreter
runtime
garbage collector
standard library
package manager
build system
debugger
language server
formatter
documentation generator
IDE tooling
interoperability layers
Language design

Fusion is still young.

Discussions about language design are especially valuable.

When proposing a language feature, please explain:

What problem does it solve?
Why should Fusion solve it?
What does the syntax look like?
What are the semantics?
How does it interact with the type system?
How would the compiler implement it?
What alternatives were considered?
Does it increase or reduce language complexity?

Prefer concrete examples over abstract arguments.

Interoperability contributions

Interoperability is one of Fusion's primary goals.

Experts from other ecosystems are particularly welcome.

For example:

Rust developers can help design Rust integration.
C++ developers can help with C++ ABI and binding challenges.
.NET developers can help with CLR integration.
Java developers can help with JVM integration.
Dart developers can help with Dart runtime integration.
Swift developers can help with Swift interoperability.
Go developers can help with Go runtime and ABI considerations.

We want integrations to be designed with people who understand the ecosystems being connected.

Documentation

Documentation is a first-class contribution.

You can help with:

tutorials
language documentation
examples
API documentation
compiler documentation
architecture documentation
interoperability guides
troubleshooting
beginner resources

A good example or explanation can be as valuable as a compiler patch.

Bug reports

Before opening a bug report:

Make sure you are using the latest version.
Search existing issues.
Verify that the problem is reproducible.
Include the smallest useful reproduction.

A useful bug report should contain:

Fusion version:
Operating system:
Expected behavior:
Actual behavior:
Steps to reproduce:
Minimal example:
Relevant error output:

Please avoid posting passwords, private source code, API keys, or other sensitive information.

Feature requests

Feature requests are welcome.

However, Fusion intentionally does not attempt to reproduce every feature of every existing programming language.

A strong feature request explains:

the problem
the proposed solution
why existing Fusion functionality is insufficient
examples
implementation considerations
interoperability implications
Pull requests

Before submitting a pull request:

Create a branch for your change.
Keep the change focused.
Add or update tests where appropriate.
Update documentation when behavior changes.
Make sure existing tests pass.
Write a clear commit message.
Explain the reasoning behind non-obvious changes.

Avoid combining unrelated changes into one pull request.

Code style

Follow the conventions already established in the part of the repository you are modifying.

For Fusion language examples, prefer the official style:

indentation-based structure
type inference when the type is obvious
const for values that never change
enum + match instead of unnecessarily long conditional chains
iterator operations for collection transformations

Do not introduce a new style merely because another language uses it.

Design philosophy

Contributors should keep the project's core principles in mind.

Expand, don't replace

Fusion should connect existing ecosystems rather than unnecessarily recreate them.

Minimize complexity

Every language feature has a maintenance cost.

A feature should justify its complexity.

Interoperability matters

New features should be considered in terms of how they interact with external ecosystems.

Developer experience matters

Fusion should remain approachable for beginners while providing advanced capabilities for experienced developers.

Be realistic

Fusion has ambitious long-term goals.

Ambition is valuable, but implementation should proceed incrementally.

If something is technically difficult, document the difficulty rather than pretending it has already been solved.

Community contributions

Fusion is intended to be community-driven.

We welcome:

independent contributors
students
educators
compiler engineers
language designers
library maintainers
open-source maintainers
infrastructure engineers
enterprise developers
researchers

Different perspectives are valuable.

Commercial contributions

Commercial organizations are welcome to participate in the project.

Companies may contribute:

engineering resources
infrastructure
integrations
testing
documentation
developer tooling
sponsorship

Commercial participation does not require the core project to become closed source.

Questions and discussions

If you are unsure whether an idea belongs in Fusion, start a discussion before implementing a large change.

For significant architectural changes, early discussion can prevent wasted work.

License

By contributing to Fusion, you agree that your contributions will be licensed under the same license as the project unless otherwise explicitly agreed.

Fusion's default software license is the MIT License.

See LICENSE.

Thank you

Fusion is attempting something ambitious:

Build bridges between programming communities instead of building another isolated island.

Every contribution helps move that idea forward.

Thank you for contributing.