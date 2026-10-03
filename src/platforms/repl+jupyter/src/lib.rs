//! REPL/Jupyter integration scaffold.

pub const PLUGIN_ID: &str = "platform.repl-jupyter";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveMode {
    Repl,
    Jupyter,
}

pub struct Session {
    pub mode: InteractiveMode,
}

impl Session {
    pub fn new(mode: InteractiveMode) -> Self {
        Self { mode }
    }

    pub fn execute(&self, _source: &str) -> Result<(), &'static str> {
        Err("interactive execution is not implemented")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_can_be_constructed() {
        let session = Session::new(InteractiveMode::Repl);
        assert_eq!(session.mode, InteractiveMode::Repl);
    }
}