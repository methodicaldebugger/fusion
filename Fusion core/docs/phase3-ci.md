# Phase 3 native integration CI gate

The workflow `.github/workflows/phase3-native-integration.yml` provides the automated Phase 3 quality gate on GitHub Actions.

## What the required job checks

The single job is named `required-phase3-gate` under the workflow **Phase 3 native integration**. It:

1. Installs a stable Rust toolchain, Clang, curl, and Python 3.
2. Checks formatting for the Fusion compiler and Foundry manifests.
3. Runs all Rust targets for the Fusion compiler crate.
4. Runs all Rust targets for the Foundry crate.
5. Runs `scripts/phase3-real-toolchain-smoke.sh`, which builds both real binaries, exercises the three-package struct/enum fixture through Foundry in both Debug and Release profiles, executes both native programs, verifies provenance and artifact hashes, checks warm-build stability in both profiles, and confirms that a Release build does not mutate Debug outputs.
6. Uploads CI toolchain details, smoke logs, and the retained fixture build (including executable and provenance when the smoke test reaches that stage) as a run artifact, including on failure.

The smoke script accepts `PHASE3_ARTIFACT_DIR` to preserve evidence and `PHASE3_KEEP_WORKSPACE=1` to retain the temporary project. CI sets both. The retained workspace can contain compiler outputs and build metadata; it is generated from checked-in test fixtures and should not contain secrets.

## Required branch-protection check

To make the workflow mandatory, configure the repository's branch protection or ruleset after the workflow has run at least once. Select this required status check:

`Phase 3 native integration / required-phase3-gate`

GitHub repository rules are an administrator setting and cannot be enabled by this source change alone. The workflow is triggered for pushes and pull requests that modify `Fusion core/**` or the workflow itself, and can also be run manually with `workflow_dispatch`.

## Evidence artifact

Each run uploads `phase3-native-integration-<run_id>-<attempt>` for 30 days. It includes:

- Rust, Cargo, Clang, target-triple, and Python versions.
- Full real-toolchain smoke output.
- Smoke exit status and executable digest, if produced.
- The retained three-package project, Debug and Release native executables, package cache records, and each profile's `.foundry-build-provenance.toml`, if the run reaches the native build stage.

A passing CI job is the first authoritative confirmation that the Rust test suites and real native integration harness execute successfully on the configured Linux runner. Local source inspection and archive checks are not substitutes for that run.

The native smoke verifier also checks per-package `foundry.lock` SHA-256 values recorded in provenance format v5. Lockfiles use format v3 and pin registry origin, path source digest, and Git commit identity.


## Phase 3 acceptance checklist

Use this checklist to decide whether Phase 3 is actually complete. A source archive or a locally successful static check is not a substitute for the required CI evidence.

| Acceptance item | Required evidence | Current status in this source snapshot |
|---|---|---|
| Compiler and Foundry formatting | Both `cargo fmt --check` commands pass | Pending Rust-equipped CI run |
| Compiler regression suite | `cargo test --all-targets` passes for `Fusion core/Cargo.toml` | Pending Rust-equipped CI run |
| Foundry regression suite | `cargo test --all-targets` passes for `foundry/Cargo.toml` | Pending Rust-equipped CI run |
| Real multi-package native build | The three-package fixture builds with the real Fusion compiler and Foundry | Harness implemented; execution pending CI |
| Native runtime behavior | Debug and Release executables both print exactly `7`, `9`, `failed` | Harness assertions implemented; execution pending CI |
| Build reproducibility and isolation | Warm builds preserve executable bytes; Release does not mutate Debug outputs | Harness assertions implemented; execution pending CI |
| Provenance integrity | Compiler, linker, package artifacts/interfaces, ordered link inputs, lockfiles and executable hashes all verify in both profiles | Debug verifier implemented; Release verifier strengthened in this increment; execution pending CI |
| Required repository gate | `Phase 3 native integration / required-phase3-gate` is enabled in branch protection/rulesets | Repository administrator action required after the workflow has run |

### Explicit completion boundary

The integration gate validates the implemented package/build path; it does not prove every planned language feature is complete. The roadmap still records work outside this gate, including making the HIR the sole compiler authority, completing unified type resolution and generic substitution, closing remaining cross-package ABI cases, and completing type-level namespace semantics. The Phase 3 owner should decide whether those are required for the Phase 3 release scope or explicitly move them to a follow-on milestone. Do not mark Phase 3 complete while any item designated release-blocking remains open.

### Release decision

Phase 3 can be marked **release-verified** only after the required workflow has a green run on the target branch, both Rust test suites and formatting checks pass, the retained evidence artifact is reviewed, and the required status check is configured. If the native gate passes but roadmap items above remain intentionally deferred, record the agreed scope boundary in `PHASE_STATUS.md` rather than silently treating them as complete.
