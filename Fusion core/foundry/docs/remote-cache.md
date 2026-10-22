# Remote build cache (initial HTTP protocol)

Set `FOUNDRY_REMOTE_CACHE` to an HTTP(S) endpoint. Foundry requests `GET <endpoint>/<fingerprint>` to restore a raw build artifact and uses `PUT <endpoint>/<fingerprint>` with the raw artifact body to publish one. A failed restore is treated as a cache miss; a failed publish emits a warning and does not fail a successful local build.

The endpoint must enforce its own access controls and storage policy. This first implementation does not yet authenticate, verify a server-provided artifact digest, or support remote caching of dependency/build intermediate artifacts.
