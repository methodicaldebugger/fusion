When fusion has: 
JIT/AOT + 
REPL/Jupyter 
+ C integration

Let us say that at this point fusion 2.0 is completed
Then this project will need assistance from larger corporations.

Fusion should not contain all integrations.
There will be a downloadable adapter/plugin system for every new language.
That way the community can build integrations without modifying the Fusion compiler every time.

Suppose Fusion 2.0 gives you:

                 Fusion 2.0
                     │
       ┌─────────────┼─────────────┐
       │             │             │
      REPL         Jupyter       IDE
       │             │             │
       └─────────────┼─────────────┘
                     │
              Fusion compiler
                /          \
              JIT           AOT
               │             │
               └──────┬──────┘
                      │
                    LLVM
                      │
                 native code
                      │
                  C ecosystem

That would already be a real, useful language.

Bejond that adding new languages should feel like adding Fusion packages. For example:
fusion add rust:some-library
fusion add maven:some-library
fusion add nuget:SomeLibrary


Fusion's package manager should ideally make foreign dependencies feel like Fusion dependencies.
We're trying to create: "An interoperability platform with a language on top of it."
The interoperability layer makes the platform valuable.
And the adapters/bridges allow the platform to grow without requiring the Fusion core team to personally implement every programming ecosystem in existence.