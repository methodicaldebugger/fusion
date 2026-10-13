//! Reserved for allocation-related rules.
//!
//! Useful future checks once Fusion has AST/type information:
//! - repeated allocations inside loops;
//! - avoidable clones/copies;
//! - collection growth without capacity hints;
//! - temporary strings in hot paths.
//!
//! These should be implemented against typed AST/HIR and, where possible,
//! validated with profiling rather than reported from fragile text heuristics.
