Toolchain discovery and foreign-build orchestration.

The orchestrator owns external compilers, linkers, SDKs and runtimes. It
consumes the neutral `fusion-interop-model` rather than putting foreign
toolchain details into the Fusion parser or Foundry dependency resolver.
Coordinates plugin availability with builds, tests, analysis and other development tools.


Core responsibilities of the toolchain orchestrator include:

Workflow coordination
Coordinates the compiler, static analyser, formatter, test runner, debugger and other tools, choosing the correct execution order and managing dependencies between tasks.

Build pipeline management
Coordinates compilation stages, conditional builds, incremental compilation, linking and artifact generation.

Tool and plugin integration
Discovers installed tools and plugins through Foundry, checks compatibility and invokes them through defined interfaces.

Resource and execution management
Manages parallel tasks, caching, resource limits, task cancellation and execution timeouts.

Result aggregation
Collects compiler errors, test results, static analysis findings, compatibility issues and performance reports into one unified result.

Environment and compatibility coordination
Ensures tools, plugins, targets and language versions are compatible before execution, and reports unmet requirements.