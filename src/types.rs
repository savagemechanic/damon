#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntentId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ToolId(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcedureId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effects(pub u32);
impl Effects {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const NETWORK: Self = Self(1 << 2);
    pub const PROCESS: Self = Self(1 << 3);
    pub const CREDENTIAL: Self = Self(1 << 4);
    pub const DESTRUCTIVE: Self = Self(1 << 5);
    pub const PRIVILEGED: Self = Self(1 << 6);
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeaningEdge {
    pub source: u32,
    pub relation: u16,
    pub target: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeaningGraph {
    pub intent: IntentId,
    pub target: Option<EntityId>,
    pub edges: Vec<MeaningEdge>,
    pub confidence: u8,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub tool: ToolId,
    pub target: Option<EntityId>,
    pub effects: Effects,
    pub args: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}
