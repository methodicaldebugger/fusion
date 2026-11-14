#!/usr/bin/env bash
# Build Fusion + Foundry with the local Rust toolchain, then exercise the real
# three-package struct/enum fixture through Foundry's production build pipeline.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FIXTURE="$ROOT/tests/fixtures/phase3-cross-package-foundry"
ARTIFACT_DIR="${PHASE3_ARTIFACT_DIR:-}"
if [[ -n "$ARTIFACT_DIR" ]]; then
  mkdir -p "$ARTIFACT_DIR"
  {
    printf 'Fusion Phase 3 real-toolchain smoke run\n'
    printf 'UTC start: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf 'Host: %s\n' "$(uname -a)"
  } > "$ARTIFACT_DIR/run-metadata.txt"
fi
WORKSPACE=""
cleanup() {
  local status=$?
  if [[ -n "$ARTIFACT_DIR" ]]; then
    mkdir -p "$ARTIFACT_DIR"
    if [[ -n "$WORKSPACE" && -d "$WORKSPACE" ]]; then
      rm -rf "$ARTIFACT_DIR/workspace"
      cp -R "$WORKSPACE" "$ARTIFACT_DIR/workspace"
    fi
    {
      if [[ $status -eq 0 ]]; then
        grep -q '^result=PASS$' "$ARTIFACT_DIR/run-metadata.txt" 2>/dev/null || printf 'result=PASS\n' >> "$ARTIFACT_DIR/run-metadata.txt"
      else
        printf 'result=FAIL\n' >> "$ARTIFACT_DIR/run-metadata.txt"
      fi
      printf 'exit_status=%s\n' "$status"
      printf 'workspace_retained=%s\n' "$([[ "${PHASE3_KEEP_WORKSPACE:-0}" == "1" ]] && echo true || echo false)"
      printf 'UTC finish: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    } >> "$ARTIFACT_DIR/run-metadata.txt"
  fi
  if [[ -n "$WORKSPACE" && -d "$WORKSPACE" ]]; then
    if [[ "${PHASE3_KEEP_WORKSPACE:-0}" == "1" ]]; then
      echo "Keeping integration workspace: $WORKSPACE"
    else
      rm -rf "$WORKSPACE"
    fi
  fi
  return "$status"
}
trap cleanup EXIT

for tool in cargo rustc clang python3; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "error: required tool '$tool' was not found in PATH" >&2
    case "$tool" in
      cargo|rustc) echo "Install a Rust toolchain (for example, via rustup) and retry." >&2 ;;
      clang) echo "Install LLVM/Clang and ensure clang is in PATH." >&2 ;;
      python3) echo "Install Python 3.11+ (tomllib is used to inspect provenance)." >&2 ;;
    esac
    exit 2
  fi
done

if ! python3 -c 'import tomllib' >/dev/null 2>&1; then
  echo "error: Python 3.11+ with tomllib is required" >&2
  exit 2
fi

FUSION_MANIFEST="$ROOT/Cargo.toml"
FOUNDRY_MANIFEST="$ROOT/foundry/Cargo.toml"
FUSION_BIN="$ROOT/target/debug/fusion"
FOUNDRY_BIN="$ROOT/foundry/target/debug/foundry"

printf '%s\n' '== Toolchain ==' 
rustc --version
cargo --version
clang --version | head -n 1

printf '%s\n' '== Build Fusion compiler =='
cargo build --manifest-path "$FUSION_MANIFEST" --bin fusion

printf '%s\n' '== Run real compiler aggregate ABI integration test =='
cargo test --manifest-path "$FUSION_MANIFEST" --lib package_aggregate_runtime_abi_crosses_module_boundaries -- --nocapture

printf '%s\n' '== Run Foundry orchestration regression test =='
cargo test --manifest-path "$FOUNDRY_MANIFEST" --lib build_inner_runs_package_compile_validate_link_and_reuses_successful_build -- --nocapture

printf '%s\n' '== Build Foundry =='
cargo build --manifest-path "$FOUNDRY_MANIFEST" --bin foundry

if [[ ! -x "$FUSION_BIN" || ! -x "$FOUNDRY_BIN" ]]; then
  echo "error: expected compiler binaries were not produced" >&2
  exit 1
fi

printf '%s\n' '== Compiler/linker identity =='
"$FUSION_BIN" --version
"$FOUNDRY_BIN" version

WORK_PARENT="${TMPDIR:-/tmp}"
WORKSPACE="$(mktemp -d "$WORK_PARENT/fusion-phase3-real-build.XXXXXX")"
cp -R "$FIXTURE" "$WORKSPACE/project"

printf '%s\n' '== Foundry multi-package native build =='
(
  cd "$WORKSPACE/project/app"
  FUSION_COMPILER="$FUSION_BIN" "$FOUNDRY_BIN" build
)

EXECUTABLE="$WORKSPACE/project/app/target/debug/app"
PROVENANCE="$WORKSPACE/project/app/target/debug/.foundry-build-provenance.toml"
if [[ ! -x "$EXECUTABLE" ]]; then
  echo "error: Foundry did not produce executable: $EXECUTABLE" >&2
  exit 1
fi
if [[ ! -s "$PROVENANCE" ]]; then
  echo "error: Foundry did not produce build provenance: $PROVENANCE" >&2
  exit 1
fi

printf '%s\n' '== Execute native program =='
ACTUAL_OUTPUT="$("$EXECUTABLE")"
EXPECTED_OUTPUT=$'7\n9\nfailed'
if [[ "$ACTUAL_OUTPUT" != "$EXPECTED_OUTPUT" ]]; then
  echo "error: unexpected native output" >&2
  printf 'expected:\n%s\nactual:\n%s\n' "$EXPECTED_OUTPUT" "$ACTUAL_OUTPUT" >&2
  exit 1
fi
printf '%s\n' "$ACTUAL_OUTPUT"

printf '%s\n' '== Verify build provenance and cached package artifacts =='
PHASE3_EXECUTABLE="$EXECUTABLE" \
PHASE3_PROVENANCE="$PROVENANCE" \
PHASE3_COMPILER="$FUSION_BIN" \
PHASE3_OUTPUT_DIR="$(dirname "$EXECUTABLE")" \
python3 - <<'PY'
import hashlib
import os
import pathlib
import tomllib

exe = pathlib.Path(os.environ["PHASE3_EXECUTABLE"])
prov_path = pathlib.Path(os.environ["PHASE3_PROVENANCE"])
compiler = pathlib.Path(os.environ["PHASE3_COMPILER"]).resolve()
out_dir = pathlib.Path(os.environ["PHASE3_OUTPUT_DIR"])

sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
prov = tomllib.loads(prov_path.read_text())
assert prov.get("format_version") == 5, "unexpected provenance format"
assert prov.get("profile") == "debug", "wrong build profile in provenance"
assert prov.get("package_interface_version") == 6, "unexpected package interface version"
assert prov.get("build_fingerprint") == (out_dir / ".foundry-fingerprint").read_text().strip(), "build fingerprint/stamp mismatch"
assert pathlib.Path(prov["compiler_path"]).resolve() == compiler, "provenance compiler path mismatch"
assert prov["compiler_sha256"] == sha(compiler), "compiler binary hash mismatch"
assert prov["executable_sha256"] == sha(exe), "executable hash mismatch"

linker = pathlib.Path(prov["linker_path"])
assert linker.is_file(), "recorded linker path does not exist"
assert prov["linker_sha256"] == sha(linker), "linker binary hash mismatch"
assert prov.get("linker_target_triple"), "missing linker target triple"
assert prov.get("linker_resource_sha256"), "missing Clang resource digest"
assert prov.get("linker_search_dirs_sha256"), "missing linker search-directory digest"
assert isinstance(prov.get("linker_environment"), list), "missing linker environment provenance"

packages = {}
for item in prov.get("package_artifacts", []):
    name, digest = item.split("=", 1)
    packages[name] = digest
assert set(packages) == {"app", "model", "consumer"}, f"unexpected package artifacts: {sorted(packages)}"

records = list((out_dir / ".foundry-package-artifacts").glob("*/artifact.toml"))
records_by_name = {}
for record_path in records:
    record = tomllib.loads(record_path.read_text())
    name = record["package_name"]
    store = out_dir / ".foundry-package-artifacts"
    artifact = store / record["artifact_file"]
    interface = store / record["interface_file"]
    assert record.get("format_version") == 6, f"unexpected package record version for {name}"
    assert record.get("artifact_kind") == "llvm-package-ir-v5", f"unexpected LLVM artifact kind for {name}"
    assert artifact.resolve().is_relative_to(store.resolve()), f"artifact path escapes cache store for {name}"
    assert interface.resolve().is_relative_to(store.resolve()), f"interface path escapes cache store for {name}"
    assert artifact.is_file(), f"missing package LLVM artifact for {name}"
    assert interface.is_file(), f"missing package interface for {name}"
    assert interface.read_text().startswith("FUSION-PACKAGE-INTERFACE\t6\n"), f"unexpected package interface version for {name}"
    assert sha(artifact) == record["artifact_sha256"], f"package artifact hash mismatch for {name}"
    assert sha(interface) == record["interface_sha256"], f"package interface hash mismatch for {name}"
    assert sha(artifact) == packages[name], f"provenance package artifact hash mismatch for {name}"
    records_by_name[name] = record
assert set(records_by_name) == set(packages), "package cache records do not match provenance"

lockfiles = {}
for item in prov.get("package_lockfiles", []):
    name, digest = item.split("=", 1)
    lockfiles[name] = digest
assert set(lockfiles) == set(packages), f"unexpected package lockfiles: {sorted(lockfiles)}"
for name, digest in lockfiles.items():
    lock_path = out_dir.parents[2] / name / "foundry.lock"
    assert lock_path.is_file(), f"missing lockfile for {name}: {lock_path}"
    assert sha(lock_path) == digest, f"lockfile hash mismatch for {name}"
    lock = tomllib.loads(lock_path.read_text())
    assert lock.get("version") == 3, f"unexpected lockfile format for {name}"

inputs = prov.get("link_inputs", [])
assert len(inputs) == 4, f"expected entrypoint plus three package inputs, got {len(inputs)}"
assert inputs[0].startswith("0:entrypoint="), "entrypoint must be the first ordered link input"
input_packages = []
for item in inputs[1:]:
    label, digest = item.split("=", 1)
    assert label.split(":", 1)[0].isdigit(), f"invalid link input ordinal: {label}"
    name = label.split("package:", 1)[1]
    assert packages.get(name) == digest, f"ordered link input hash mismatch for {name}"
    input_packages.append(name)
assert set(input_packages) == {"app", "model", "consumer"}, f"unexpected link input packages: {input_packages}"
assert input_packages.index("model") < input_packages.index("consumer"), "dependency must precede consumer in link order"
assert input_packages.index("consumer") < input_packages.index("app"), "consumer must precede application in link order"
ordered_digest = hashlib.sha256()
for item in inputs:
    encoded = item.encode()
    ordered_digest.update(len(encoded).to_bytes(8, "little"))
    ordered_digest.update(encoded)
assert prov.get("link_inputs_sha256") == ordered_digest.hexdigest(), "ordered link-input digest mismatch"

print("provenance format: 5")
print("package lockfile hashes: verified")
print("compiler SHA-256: verified")
print("linker SHA-256: verified")
print("package LLVM/interface hashes: verified for app, model, consumer")
print("ordered link inputs: verified")
print("executable SHA-256: verified")
PY

printf '%s\n' '== Warm build reuse =='
BEFORE="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$EXECUTABLE")"
(
  cd "$WORKSPACE/project/app"
  FUSION_COMPILER="$FUSION_BIN" "$FOUNDRY_BIN" build
)
AFTER="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$EXECUTABLE")"
if [[ "$BEFORE" != "$AFTER" ]]; then
  echo "error: warm build changed executable bytes" >&2
  exit 1
fi

# Release must occupy an independent output/provenance namespace and must not
# mutate the already-verified debug executable or its provenance document.
DEBUG_AFTER_WARM="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$EXECUTABLE")"
DEBUG_PROVENANCE="$WORKSPACE/project/app/target/debug/.foundry-build-provenance.toml"
DEBUG_PROVENANCE_BEFORE="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$DEBUG_PROVENANCE")"

printf '%s\n' '== Release multi-package native build =='
(
  cd "$WORKSPACE/project/app"
  FUSION_COMPILER="$FUSION_BIN" "$FOUNDRY_BIN" build --release
)
RELEASE_EXECUTABLE="$WORKSPACE/project/app/target/release/app"
RELEASE_PROVENANCE="$WORKSPACE/project/app/target/release/.foundry-build-provenance.toml"
if [[ ! -x "$RELEASE_EXECUTABLE" || ! -s "$RELEASE_PROVENANCE" ]]; then
  echo "error: release build did not produce executable and provenance" >&2
  exit 1
fi
RELEASE_OUTPUT="$("$RELEASE_EXECUTABLE")"
if [[ "$RELEASE_OUTPUT" != "$EXPECTED_OUTPUT" ]]; then
  echo "error: release executable produced unexpected output" >&2
  printf 'expected:\n%s\nactual:\n%s\n' "$EXPECTED_OUTPUT" "$RELEASE_OUTPUT" >&2
  exit 1
fi
python3 - "$RELEASE_EXECUTABLE" "$RELEASE_PROVENANCE" "$FUSION_BIN" "$DEBUG_AFTER_WARM" "$DEBUG_PROVENANCE_BEFORE" "$EXECUTABLE" "$DEBUG_PROVENANCE" <<'PYRELEASE'
import hashlib, pathlib, sys, tomllib
release_exe, release_prov, compiler = map(pathlib.Path, sys.argv[1:4])
debug_hash, debug_prov_hash = sys.argv[4:6]
debug_exe, debug_prov = map(pathlib.Path, sys.argv[6:8])
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
prov = tomllib.loads(release_prov.read_text())
assert prov.get('format_version') == 5, 'unexpected release provenance format'
assert prov.get('profile') == 'release', 'release provenance has wrong profile'
assert prov.get('package_interface_version') == 6, 'release package interface version mismatch'
assert prov.get('executable_sha256') == sha(release_exe), 'release executable hash mismatch'
assert pathlib.Path(prov['compiler_path']).resolve() == compiler.resolve(), 'release compiler path mismatch'
assert prov.get('compiler_sha256') == sha(compiler), 'release compiler hash mismatch'
linker = pathlib.Path(prov['linker_path'])
assert linker.is_file(), 'recorded release linker path does not exist'
assert prov.get('linker_sha256') == sha(linker), 'release linker hash mismatch'
assert prov.get('linker_target_triple'), 'release provenance lacks linker target triple'
assert prov.get('linker_resource_sha256'), 'release provenance lacks linker resource digest'
assert prov.get('linker_search_dirs_sha256'), 'release provenance lacks linker search-directory digest'
assert isinstance(prov.get('linker_environment'), list), 'release provenance lacks linker environment record'
release_out = release_prov.parent
assert prov.get('build_fingerprint') == (release_out / '.foundry-fingerprint').read_text().strip(), 'release fingerprint/stamp mismatch'

release_packages = {}
for item in prov.get('package_artifacts', []):
    name, digest = item.split('=', 1)
    release_packages[name] = digest
assert set(release_packages) == {'app', 'model', 'consumer'}, 'release package artifact provenance is incomplete'
store = release_out / '.foundry-package-artifacts'
release_records = {}
for record_path in store.glob('*/artifact.toml'):
    record = tomllib.loads(record_path.read_text())
    name = record['package_name']
    artifact = store / record['artifact_file']
    interface = store / record['interface_file']
    assert record.get('format_version') == 6, f'unexpected release package record version for {name}'
    assert record.get('artifact_kind') == 'llvm-package-ir-v5', f'unexpected release artifact kind for {name}'
    assert artifact.resolve().is_relative_to(store.resolve()), f'release artifact path escapes cache for {name}'
    assert interface.resolve().is_relative_to(store.resolve()), f'release interface path escapes cache for {name}'
    assert artifact.is_file() and interface.is_file(), f'missing release package cache object for {name}'
    assert interface.read_text().startswith('FUSION-PACKAGE-INTERFACE\t6\n'), f'release package interface version mismatch for {name}'
    assert sha(artifact) == record['artifact_sha256'], f'release package artifact hash mismatch for {name}'
    assert sha(interface) == record['interface_sha256'], f'release package interface hash mismatch for {name}'
    assert sha(artifact) == release_packages[name], f'release package provenance hash mismatch for {name}'
    release_records[name] = record
assert set(release_records) == set(release_packages), 'release package cache records do not match provenance'

release_locks = {}
for item in prov.get('package_lockfiles', []):
    name, digest = item.split('=', 1)
    release_locks[name] = digest
assert set(release_locks) == set(release_packages), 'release lockfile provenance is incomplete'
project_root = release_prov.parent.parents[2]
for name, digest in release_locks.items():
    lock_path = project_root / name / 'foundry.lock'
    assert lock_path.is_file(), f'missing release lockfile for {name}'
    assert sha(lock_path) == digest, f'release lockfile hash mismatch for {name}'
    assert tomllib.loads(lock_path.read_text()).get('version') == 3, f'release lockfile format mismatch for {name}'

inputs = prov.get('link_inputs', [])
assert len(inputs) == 4 and inputs[0].startswith('0:entrypoint='), 'release ordered link inputs are incomplete'
input_packages = []
for item in inputs[1:]:
    label, digest = item.split('=', 1)
    assert label.split(':', 1)[0].isdigit(), f'invalid release link input ordinal: {label}'
    name = label.split('package:', 1)[1]
    assert release_packages.get(name) == digest, f'release ordered link input hash mismatch for {name}'
    input_packages.append(name)
assert set(input_packages) == {'app', 'model', 'consumer'}, 'unexpected release link package set'
assert input_packages.index('model') < input_packages.index('consumer') < input_packages.index('app'), 'release dependency link order is invalid'
ordered = hashlib.sha256()
for item in inputs:
    encoded = item.encode()
    ordered.update(len(encoded).to_bytes(8, 'little'))
    ordered.update(encoded)
assert prov.get('link_inputs_sha256') == ordered.hexdigest(), 'release ordered link-input digest mismatch'

assert sha(debug_exe) == debug_hash, 'release build modified debug executable'
assert sha(debug_prov) == debug_prov_hash, 'release build modified debug provenance'
print('release package artifacts, interfaces, lockfiles, linker and ordered inputs: verified')
PYRELEASE

printf '%s\n' '== Release warm-build stability =='
RELEASE_BEFORE="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$RELEASE_EXECUTABLE")"
(
  cd "$WORKSPACE/project/app"
  FUSION_COMPILER="$FUSION_BIN" "$FOUNDRY_BIN" build --release
)
RELEASE_AFTER="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$RELEASE_EXECUTABLE")"
if [[ "$RELEASE_BEFORE" != "$RELEASE_AFTER" ]]; then
  echo "error: release warm build changed executable bytes" >&2
  exit 1
fi

printf '%s\n' 'PASS: real Fusion compiler + Foundry debug/release multi-package native builds, execution, provenance, profile isolation, and warm-build stability.'
if [[ -n "$ARTIFACT_DIR" ]]; then
  printf '%s\n' 'result=PASS' >> "$ARTIFACT_DIR/run-metadata.txt"
  printf 'executable_sha256=%s\n' "$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$EXECUTABLE")" >> "$ARTIFACT_DIR/run-metadata.txt"
  printf 'provenance_path=workspace/project/app/target/debug/.foundry-build-provenance.toml\n' >> "$ARTIFACT_DIR/run-metadata.txt"
fi
