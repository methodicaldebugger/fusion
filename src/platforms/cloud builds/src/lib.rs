//! Cloud build integration scaffold.

pub const PLUGIN_ID: &str = "platform.cloud-builds";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Executor {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy)]
pub struct BuildRequest<'a> {
    pub project: &'a str,
    pub executor: Executor,
}

pub struct BuildResult {
    pub success: bool,
}

pub fn execute(_request: BuildRequest<'_>) -> Result<BuildResult, &'static str> {
    Err("cloud build execution is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_execution_is_not_claimed_as_implemented() {
        let request = BuildRequest {
            project: "example",
            executor: Executor::Remote,
        };

        assert!(execute(request).is_err());
    }
}