Send me files you have made by visiting the link: https://fusion.ifree.page


When fusion has:
achieved self-hoasting and C interoperability
I stop building it, the community may continue.

Fusion needs to make implementing those integrations possible without redesigning the language, such as:
JIT/AOT + REPL/Jupyter + actors system(elixir/erlang), 
UI framework(like flutter), SQLite, cloud builds,
5 layers of integration for every language out there, including those not yet created.



Fusion will create/have folders as follows:

fusion/
├── compiler/
├── runtime/
├── toolchain/
├── interoperability/
│   ├── native_abi/
│   ├── managed_runtime/
│   ├── embedded_runtime/
│   ├── process_rpc/
│   └── wasm/
├── languages/
│   ├── c/
│   ├── cpp/
│   ├── csharp/
│   ├── go/
│   ├── swift/
│   ├── dart/
│   └── java/
├── platform/
│   ├── jit/
│   ├── aot/
│   ├── repl/
│   ├── jupyter/
│   ├── sqlite/
│   ├── actors/
│   ├── ui/
│   └── cloud/
└── docs/

The user will be able to drop new source_language_support and library_integrations files as they are created by the fusion community.

An ambitious future
One possible future Fusion workflow could look like:

Developer writes Fusion
        │
        ▼
     Profile
        │
        ▼
Compiler detects expensive function
        │
        ▼
Suggestion:
"simulate() consumes 68% of CPU."
        │
        ├── Optimize Fusion
        ├── Move to Rust
        ├── Move to GPU
        └── Parallelize

A future IDE could potentially generate the required bindings automatically.

THERE ARE MANY THINGS ON THE INTERNET THAT CAN HELP BUILD FUSION FASTER:
FLUTTER the framework UI from dart(programming language), sqlite, skia-main, LLVM. Use them.


Fusion should not contain all integrations.
There will be a downloadable adapter/plugin system for every new language.
That way the community can build integrations without modifying the Fusion compiler every time.


Adding new languages should feel like adding Fusion packages. For example:
fusion add rust:some-library
fusion add maven:some-library
fusion add nuget:SomeLibrary


Fusion's package manager should ideally make foreign dependencies feel like Fusion dependencies.
We're trying to create: "An interoperability platform with a language on top of it."
The interoperability layer makes the platform valuable.
And the adapters/bridges allow the platform to grow without requiring the Fusion core team to personally implement every programming ecosystem in existence.



Fusion becomes the application-level language while other ecosystems become implementation-level resources/assets.
A Fusion developer shouldn't necessarily ask: “Which language should I use?”
They could ask: “Which implementation/ecosystem is best for this particular capability?”
Fusion becomes the application-level language while other ecosystems become implementation-level resources/assets. Fusion itself must be a good language and Fusion's interoperability makes it useful. A beginner learns fusion, but masters gain capabilities originating from many ecosystems.

If Fusion succeeds, it might look like: “People stopped caring which language their dependency was written in.”
Tutorials and knowledge in many/all programing languages will become useful to a fusion developer.                       

To a developer, he chooses which plugins he will download(if any) and then writes the program in fusion(+ other languages with plugins). This means fusion can call many foreign libraries and embeed other languages via plugins. For example the C plugin will enable the developer to write and compile a C file and of course a SEPARATE fusion file.

I'd avoid calling the executable component literally the "Fusion compiler" when it's compiling C/C#.

A cleaner architecture might be: Fusion Toolchain

Fusion Toolchain
├── Fusion compiler
├── C integration → C compiler
├── C# integration → .NET compiler
├── Rust integration → Rust compiler
└── ...

Then the user experience is unified even though the underlying language compilers remain specialized.

That would let Fusion do something pretty unusual:
You don't have to write Fusion to use Fusion.

You could use Fusion as the build/run/debug environment for an existing C project, an existing C# project, or eventually a project containing many languages.
You could have a .fusion file, use foreign source through fusion(main.c->fusion->C integration) or a mixed project with a .fusion and a .c file.