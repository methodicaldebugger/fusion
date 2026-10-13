//! UI framework integration scaffold.

pub const PLUGIN_ID: &str = "platform.ui-framework";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiTarget {
    Desktop,
    Mobile,
    Web,
}

pub fn supports(_target: UiTarget) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_ui_target_is_claimed_as_supported() {
        assert!(!supports(UiTarget::Desktop));
        assert!(!supports(UiTarget::Mobile));
        assert!(!supports(UiTarget::Web));
    }
}