//! SQLite platform integration scaffold.

pub const PLUGIN_ID: &str = "platform.sqlite";

pub struct Connection {
    _private: (),
}

impl Connection {
    pub fn open(_path: &str) -> Result<Self, &'static str> {
        Err("SQLite support is not implemented")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_is_not_claimed_as_implemented() {
        assert!(Connection::open(":memory:").is_err());
    }
}