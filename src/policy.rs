use crate::types::{Action, Effects};
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub allowed: Effects,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            allowed: Effects::READ.union(Effects::PROCESS),
        }
    }
}
impl Policy {
    pub fn check(&self, action: &Action) -> Result<(), String> {
        if self.allowed.contains(action.effects) {
            Ok(())
        } else {
            Err(format!(
                "action requires effects mask {:#x}, allowed is {:#x}",
                action.effects.0, self.allowed.0
            ))
        }
    }
}
