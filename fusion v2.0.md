When fusion has: 
JIT/AOT + 
REPL/Jupyter 
+ C integration
Create a built-in UI framework, similar to what Flutter did for Dart(that way you write: the backend, the frontend, desktop apps, mobile apps).
SQLite support in the standard library.
Cloud builds
Erlang / Elixir: "Let it crash philosophy" / actors system
Don't make every component capable of recovering from every possible failure. Isolate components so that a failure can be contained and the component restarted. This feature is only available in fusion 2.0.
Actors are especially interesting for distributed systems(web servers, messaging systems, multiplayer games, distributed services, telecommunications, fault-tolerant systems). Actors don't necessarily need to live in the same process—or even the same machine.

Fusion will create five folders as following:
Fusion/
├── fusion/
├── runtime_plugins/
│   └── actors/
├── library_integrations/
│   ├── sqlite/
│   └── rust/
├── source_language_support/
│   └── c/
└── frameworks/
    └── flutter/

The user will be able to drop new source_language_support and library_integrations files as they are created by the fusion community.

THERE ARE MANY THINGS ON THE INTERNET THAT CAN HELP BUILD FUSION FASTER:
FLUTTER the framework UI from dart(programming language), sqlite, skia-main, sdk-main, LLVM.

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


The following projects may be built independently, isolated and replacable by the user:
| Project                               | Difficulty | Relative size | What you're actually building                                                   |
| ------------------------------------- | ---------: | ------------: | ------------------------------------------------------------------------------- |
| 🟢 **REPL**                           |       3/10 |         Small | Interactive Fusion execution environment                                        |

| 🟢 **Jupyter integration**            |       4/10 |  Small–medium | Fusion kernel that speaks Jupyter's protocol                                    |

| 🟡 **Fusion UI on existing renderer** |     6–7/10 |         Large | Widgets, layout, events, state, accessibility, platform integration             |

| 🔴 **Actor system**                   |       7/10 |  Medium–large | Actors, mailboxes, scheduler, supervision, concurrency, eventually distribution |


Fusion 2.0 can be used to make CLI applications, Backend services, Developer tools, Desktop applications(once UI framework exists), Data processing, Scientific computing, Games and graphics, Cloud applications, Distributed systems.

| Capability          | Fusion 2.0 alone | What it needs                              |
| ------------------- | ----------------- | ------------------------------------------ |
| Application logic   | 🟢                | Language/runtime                           |
| CLI applications    | 🟢                | Standard library                           |
| Web servers         | 🟢                | Networking/HTTP libraries                  |
| Desktop UI          | 🟢*               | Fusion UI toolkit                          |
| Mobile applications | 🟢*               | Fusion mobile framework + platform tooling |
| Databases           | 🟢*               | Database drivers/implementations           |
| Compilers           | 🟢                | File/strings/AST/process libraries         |
| WebAssembly         | 🟢*               | WASM compiler target/runtime               |
| Cloud applications  | 🟢                | Networking + deployment tooling            |
+ Distributed systems