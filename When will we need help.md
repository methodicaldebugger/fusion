When fusion has: 
JIT/AOT + 
REPL/Jupyter 
+ C integration

This project will need assistance from larger corporations.

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