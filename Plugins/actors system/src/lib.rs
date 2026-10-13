//! Actor system scaffold.

pub const PLUGIN_ID: &str = "platform.actors";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActorId(pub u64);

pub struct ActorSystem {
    next_id: u64,
}

impl ActorSystem {
    pub fn new() -> Self {
        Self { next_id: 0 }
    }

    pub fn spawn(&mut self) -> ActorId {
        let id = ActorId(self.next_id);
        self.next_id += 1;
        id
    }

    pub fn send(&self, _actor: ActorId) -> Result<(), &'static str> {
        Err("actor message delivery is not implemented")
    }
}

impl Default for ActorSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_ids_are_unique() {
        let mut system = ActorSystem::new();

        assert_ne!(system.spawn(), system.spawn());
    }
}