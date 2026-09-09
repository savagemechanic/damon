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
        if crate::tools::capability_for_tool(action.tool) != Some(action.capability) {
            return Err("tool is not the registered implementation of this capability".into());
        }
        let required = crate::tools::required_effects(action.tool)?;
        if action.effects != required {
            return Err("action effects do not match the registered tool".into());
        }
        if self.allowed.contains(required) {
            Ok(())
        } else {
            Err(format!(
                "action requires effects mask {:#x}, allowed is {:#x}",
                action.effects.0, self.allowed.0
            ))
        }
    }
}
