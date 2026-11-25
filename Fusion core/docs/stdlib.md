# Fusion 0.3 standard library additions

Everything below is implemented in the Rust interpreter using only `std` (no new crates).
Runtime failures follow the interpreter's existing convention: they stop the program with a
message (and fail a `#[test]`). Function names are deliberately prefixed to avoid clashing with
user functions.

## Text and encoding
`regex_is_match`, `regex_find`, `regex_find_all`, `regex_captures`, `regex_replace` (`$1` group
references), `regex_split` — a backtracking engine with classes, `\d \w \s \b`, anchors, groups,
alternation, `* + ? {n,m}` (lazy with `?`) and a leading `(?i)`. No lookaround or backreferences.
`utf8_valid`, `utf8_encode`, `utf8_decode` (byte arrays are arrays of 0–255),
`str_char_count`, `str_byte_len`, `str_find`, `str_slice`, `str_join`, `str_repeat`,
`str_parse_int`, `str_parse_float`.

## Data formats
`json_parse`, `json_try_parse`, `json_stringify`, `json_pretty`, `toml_parse`, `toml_stringify`.
JSON objects and TOML tables become hash maps, arrays become arrays, `null` becomes `None`.
TOML covers tables, dotted keys, arrays of tables, inline tables, multi-line strings and numbers;
dates are kept as strings.

## Crypto and randomness
`sha256`, `hmac_sha256` (hex output), `constant_time_eq`, `random_bytes_hex`,
`secure_random_int`, `secure_random_float`, `uuid4` — randomness comes from the operating system
(`/dev/urandom`, `RtlGenRandom`). The older `random_int` / `random_float` remain a fast,
non-cryptographic generator.

## Time
`date_format` (strftime subset plus `%Z` abbreviations and `%:z`), `date_parse` (ISO 8601 /
RFC 3339), `date_parts`, `date_from_parts`, `date_add_months`, `days_in_month`, `is_leap_year`,
`now_iso`, `tz_offset`, `tz_abbrev`, `tz_is_valid`, `tz_names`.
Zones are `UTC`/`Z`, fixed offsets (`+02:00`), IANA names (`Europe/Ljubljana`) or POSIX TZ strings
(`CET-1CEST,M3.5.0,M10.5.0/3`). Names resolve from the operating system's tz database when it is
installed; if unavailable, Fusion uses its bundled IANA tzdata 2025b TZif snapshot, including
historical transitions. The smaller POSIX rule table is only the last-resort fallback for zones
not present in either database. Update the bundled snapshot when shipping a new Fusion release.
A wall-clock time that occurs twice (clocks going back) resolves to its first occurrence; a time
that does not exist (clocks going forward) is read with the offset in effect before the gap.

## Networking
TCP and UDP via numeric handles: `tcp_listen`, `tcp_dial`, `tcp_accept`, `tcp_read`, `tcp_write`,
`tcp_close`, `tcp_set_timeout`, `tcp_local_port`, `tcp_peer_addr`, `udp_bind`, `udp_send_to`,
`udp_recv_from`, `udp_close`.
HTTP/1.1: `http_request`, `http_get`, `http_post` (one-shot client; returns `status`, `reason`, `headers`,
`body`), `http_client_open(url, timeout_ms)`, `http_client_request(handle, method, path, headers, body)`
and `http_client_close(handle)` (reusable persistent client session), plus `http_accept` / `http_respond`
(server: accept a request map, respond on its `conn`).
Helpers: `url_parse_query`, `url_encode`, `url_decode`.
Servers: `http_listen(host, port)` / `https_listen(host, port, cert_pem, key_pem)` start a
server whose acceptor thread handles connections concurrently and detects HTTP/1.1 versus HTTP/2
(TLS ALPN, or the HTTP/2 preface on plain TCP); `http_accept` returns the next request from any
connection and `http_respond` answers it. `tcp_listen` + `http_accept` remains a simple
HTTP/1.1-only loop.
Clients: `http://` is HTTP/1.1, `https://` negotiates `h2` or `http/1.1` with ALPN, and `h2c://`
speaks HTTP/2 over plain TCP.

**Optional Cargo features** (the default build has no dependencies):
`cargo build --features tls` adds HTTPS through `rustls` (with the Mozilla root certificates);
`--features http2` adds HPACK header compression through the `hpack` crate; use
`--features tls,http2` for HTTPS with HTTP/2. TLS is deliberately not hand-written. Without the
features, `https://`, `https_listen` and HTTP/2 return an error naming the feature.
HTTP/1.1 keep-alive is supported by `http_listen`/`https_listen` servers and the reusable
`http_client_*` session API. Requests are processed sequentially per HTTP/1.1 connection; request
pipelining is not supported. The simple `tcp_listen` + `http_accept` path still closes after one
response. HTTP/2 provides a reusable `h2_ext::ClientSession` that can have multiple streams in
flight, and `serve_with_push` for explicit server pushes. Server handlers remain synchronous, so
this is not yet a fully concurrent HTTP/2 server implementation. HTTP/2 priorities are not
implemented. **Not implemented:** streaming request/response bodies, client certificates, and a
routing framework. Bodies exposed to Fusion are text (invalid UTF-8 is replaced).

## Files, paths, processes
`path_join`, `path_parent`, `path_file_name`, `path_extension`, `path_stem`, `path_normalize`,
`path_is_absolute`, `path_is_file`, `path_is_dir`, `file_size`, `modified_time`, `copy_file`,
`rename_path`, `remove_dir`, `append_file`, `read_lines`, `walk_dir`, `is_readonly`,
`set_readonly`, `file_mode`/`set_file_mode` (Unix), `temp_dir`, `home_dir`, `env_set`, `env_vars`.
Processes: `process_output`, `process_output_input`, `process_spawn`, `process_write`,
`process_close_stdin`, `process_try_wait`, `process_kill`, `process_wait`.

## Concurrency
`task_spawn(name, args)` (one OS thread; use it for tasks that block), `pool_spawn(name, args)`
(work-stealing pool), `task_join`, `task_join_all`, `task_is_finished`, `parallel_map` (pool),
`cpu_count`, structured scopes (`scope_new`, `scope_spawn`, `scope_join`, `scope_cancel`,
`scope_is_cancelled`), channels (`channel_new(capacity)`, `channel_send`, `channel_recv`,
`channel_try_recv`, `channel_recv_timeout`, `channel_close`, `channel_len`) and atomic counters.
Each task is an OS thread with its own interpreter; arguments, results and messages are copied,
so they must be numbers, floats, strings, bools, arrays, maps or structs (no files/streams).
Actors are a mailbox channel plus a task loop (see `examples/tasks_and_actors.fusion`).
The pool has one deque per worker plus a global queue; idle workers steal the oldest job from
other workers, and a task that waits for another task runs queued jobs instead of blocking its
thread, so nested fork/join cannot exhaust the pool. `scope_join` waits for every child, so no
child outlives its scope; a failed child cancels the scope (siblings can poll
`scope_is_cancelled`) and the first error is reported. Cancellation is cooperative.
`fusion run FILE --concurrent-async` (or `FUSION_CONCURRENT_ASYNC=1`) runs `async fn` calls on the
pool and makes `await` wait for them; by default async functions still run eagerly at the call.
Concurrent async copies arguments (so they must be transferable values), runs the function in a
fresh interpreter (globals are not shared) and delivers its `print` output when awaited.
**Not implemented:** green threads or preemption, suspending/resuming `async` functions at
`await`, and async-native socket/file I/O. The current pool schedules whole interpreter calls;
it is not a green-thread executor.

**Release builds:** `Cargo.toml` sets `panic = "abort"` for `--release`. Tasks, scopes and the
test runner rely on catching panics, so with that profile a failing task or test aborts the whole
process. Use a debug build, or change the profile to `panic = "unwind"`, if you need them in
release builds.

## Testing, benchmarks, docs
`#[test]`, `#[ignore]`, `#[should_panic]`, `#[bench]` and `assert`, `assert_eq`, `assert_ne`,
`assert_msg`, `test_fail`. Commands: `fusion test FILE [--filter TEXT] [--ignored]`,
`fusion bench FILE`, `fusion doc FILE|DIR [--html] [-o OUT]`. Foundry runs them for a project:
`foundry test [--filter TEXT] [--ignored]` runs unit tests and then `tests/*.fusion`, and
`foundry doc` writes `target/doc/api.md` and `api.html` next to its manifest summary. `///` comments (and `//!` for the
module) document the declaration below them.

## Helpers
`option_is_some`, `option_unwrap`, `option_unwrap_or`, `map_get_or`.

`Some(x)`, `None`, `Ok(x)` and `Err(e)` build `Option` and `Result` values (see the README section
"Option and Result values"). Helpers: `option_is_none`, `result_is_ok`, `result_is_err`, `result_unwrap`,
`result_unwrap_err`, `result_unwrap_or`.

## Function names given as strings

`task_spawn`, `parallel_map` and the test runner take function names as strings. Because module
lowering renames every function to an internal symbol, the interpreter resolves `"name"` as a
function of the entry module and `"module.name"` as a function of another module.
