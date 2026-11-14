//! Shared configuration for the native LLVM-to-executable link step.
//! Keep this as the single source of truth for flags used by Fusion and fingerprinted by Foundry.
pub const FLAGS: &[&str] = &["-O2"];
