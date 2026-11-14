# Requested runtime work — implementation status

This note records the state of the five requested areas after this continuation. It intentionally
distinguishes implemented code from remaining engineering work; the presence of APIs does not
mean full protocol/runtime conformance.

## Implemented in this continuation

### HTTP/1.1 keep-alive
- `http_listen`/`https_listen` connection workers now process multiple sequential HTTP/1.1 requests
  on one connection when the request semantics permit persistence.
- HTTP/1.1 defaults to persistent connections; `Connection: close` closes after the response.
  HTTP/1.0 requires an explicit `Connection: keep-alive`.
- Added reusable client handles: `http_client_open`, `http_client_request`, `http_client_close`.
- The reusable client preserves response bytes read ahead, handles Content-Length, chunked bodies,
  HEAD/1xx/204/304 no-body cases, and marks close-delimited responses non-reusable.
- Request pipelining is not implemented. The simple `tcp_listen` + `http_accept` path still handles
  one request per accepted connection.

### HTTP/2 streams and server push
- Added `h2_ext::ClientSession`, allowing multiple request streams to be submitted before waiting
  for their responses. Completed responses are associated with stream IDs and can be collected in
  any order. Even stream IDs identify pushed responses.
- Added `serve_with_push`, `Push`, and `ServerResponse`, plus PUSH_PROMISE emission and basic
  reception/validation. Clients must explicitly enable push in `ClientSession::new`.
- PUSH_PROMISE header blocks are split across PUSH_PROMISE/CONTINUATION frames when necessary.
  Concurrent server handler execution, priorities, full RFC conformance testing, and a
  production-grade multiplexed server scheduler remain outstanding. The old `client_request` remains a one-shot
  convenience wrapper and disables push.

## Still not implemented

### Preemptive green threads
The current work-stealing pool schedules whole jobs, each of which executes an interpreter call.
Tasks that block still use OS threads. There is no preemptive green-thread scheduler, stackful
fiber/context switching, or timer-based task preemption. Implementing this safely requires a
well-defined safepoint/continuation model or a vetted fiber runtime and cancellation integration.

### Fully suspending async/await and asynchronous I/O
`await` still evaluates/joins the current task representation; interpreter evaluation is synchronous
and recursively executes expressions/statements on the Rust call stack. There is no resumable
interpreter frame/state machine, Future polling protocol, I/O reactor, or cancellation-safe async
resource cleanup. The `--concurrent-async` option schedules whole async functions; it does not
suspend at each await. A correct implementation requires lowering async functions into resumable
state machines and routing socket/file operations through a runtime.

### Historical timezone fallback
- Added an embedded TZif archive generated from IANA tzdata 2025b (public-domain database).
- Host zoneinfo remains preferred; embedded TZif is used when the host database is absent.
- This removes dependence on the small current-rules POSIX table for zones included in the bundled
  snapshot, including historical local-mean-time and DST transitions.
- The database must be refreshed periodically to include future political timezone changes.

## Validation limitations

Regression tests were added for persistent HTTP/1.1 reuse, HTTP/2 multiple in-flight streams,
HTTP/2 server push, and historical `Europe/Ljubljana` offsets. This environment has no `cargo`,
`rustc`, or `rustfmt`, so compilation and test execution could not be performed here. Run the
following on a machine with the Rust toolchain:

```powershell
cargo test
cargo test --features http2
cargo test --features tls
cargo test --features "tls http2"
```

The last two commands also require the optional TLS dependencies to be available to Cargo.
