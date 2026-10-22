# Pinned Fusion toolchain feed

Set `[toolchain].fusion` to an exact version and `FOUNDRY_TOOLCHAIN_FEED` to a feed URL or local directory. Foundry reads `<feed>/<version>.toml` with these fields:

```toml
version = "1.4.2"
url = "https://downloads.example/fusion-1.4.2.tar"
sha256 = "<64 hexadecimal SHA-256 characters>"
executable = "bin/fusion"
```

The archive is SHA-256 verified before extraction. `executable` is relative to the extracted version directory. `FOUNDRY_HOME` overrides the default toolchain home (`$HOME/.foundry`). Set `FOUNDRY_OFFLINE=1` to prohibit downloading a missing pinned toolchain. Explicit `FUSION_COMPILER` overrides the manifest pin.
