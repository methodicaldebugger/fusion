# Claude standard-library integration notes

## Merge strategy

The Phase 3 continuation archive is the authoritative base. Its larger, newer compiler implementation
was retained, including `module_system.rs`, HIR/name-resolution changes, `hir_body.rs`, module regression
tests, `foundry/src/package_graph.rs`, and Foundry's package-aware build fingerprint integration.

Claude's older compiler snapshots were not copied over the newer compiler. Instead, the following additive
standard-library/runtime files were ported: `conc_ext.rs`, `net_ext.rs`, `time_ext.rs`, `stdlib_ext.rs`,
`tz_ext.rs`, `tls_ext.rs`, and `h2_ext.rs`. The optional `tls` and `http2` Cargo features were added.

The interpreter's HIR-identity-aware implementation was preserved; only the opt-in concurrent-async flag,
work-pool dispatch, and pending-task `await` handling were merged. Foundry's package graph was preserved;
Claude's unit-test and API-documentation orchestration was added around it.

## Important limitations

- No Rust compiler was available during this merge, so compilation and tests are unverified.
- TLS/HTTP2 require optional features. Test the feature combinations on Windows.
- HTTP/2 implementation caveats remain: no multiplexed requests per connection or server push; the
  server handles one request at a time per connection. HTTP/1.1 keep-alive remains unsupported.
- Time-zone built-in fallback rules may be inaccurate for historical dates on systems without a tz database.
- Async remains eager by default. `--concurrent-async` opts into pool scheduling; this is not a fully
  suspending async I/O runtime. Work stealing is cooperative, not preemptive green-thread scheduling.
- Structured-scope cancellation is cooperative.
- `panic = "abort"` in release profile can terminate the process on panics instead of isolating task/test
  failures; evaluate changing to `panic = "unwind"` if release-mode panic isolation is required.
