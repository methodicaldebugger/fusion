//! JIT/AOT platform plugin scaffold.

pub const PLUGIN_ID: &str = "platform.jit-aot";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Jit,
    Aot,
}

pub fn supports(_mode: ExecutionMode, _target: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_is_not_claimed_as_implemented() {
        assert!(!supports(ExecutionMode::Jit, "unknown"));
        assert!(!supports(ExecutionMode::Aot, "unknown"));
    }
}