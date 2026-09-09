//! Capability-first resolution. Applications can be reference implementations,
//! but verified native/system/protocol implementations are always preferred.
use crate::{
    codec::{write_string, write_u16, write_u32, write_u64, Reader},
    storage,
    types::{CapabilityId, Effects, ProcedureId, ToolId},
};
use std::{collections::HashMap, io, io::Write};

pub const GIT_STATUS: CapabilityId = CapabilityId(1);
pub const GIT_DIFF: CapabilityId = CapabilityId(2);
pub const RUN_TESTS: CapabilityId = CapabilityId(3);
pub const LIST_FILES: CapabilityId = CapabilityId(4);
pub const FIND_CHANGED_FILES: CapabilityId = CapabilityId(5);
pub const INSPECT_INTERFACES: CapabilityId = CapabilityId(6);
pub const INSPECT_ROUTES: CapabilityId = CapabilityId(7);
pub const INSPECT_NEIGHBORS: CapabilityId = CapabilityId(8);
pub const LIST_SOCKETS: CapabilityId = CapabilityId(9);
pub const CAPTURE_PACKETS: CapabilityId = CapabilityId(10);
pub const TCP_CONNECT: CapabilityId = CapabilityId(11);
pub const UDP_EXCHANGE: CapabilityId = CapabilityId(12);
const MAX_CAPABILITIES: usize = 4096;
const MAX_IMPLEMENTATIONS: usize = 8192;
const MAX_DEPENDENCIES: usize = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ImplementationKind {
    Native = 1,
    Composition = 2,
    System = 3,
    Protocol = 4,
    Generated = 5,
    StructuredExternal = 6,
    ApplicationReference = 7,
    Accessibility = 8,
    Pixels = 9,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Verification {
    Unverified = 0,
    Compiled = 1,
    Tested = 2,
    Verified = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Provenance {
    BuiltIn = 1,
    Configured = 2,
    Learned = 3,
    Generated = 4,
    ApplicationReference = 5,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    pub id: CapabilityId,
    pub name: String,
    pub effects: Effects,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Implementation {
    pub capability: CapabilityId,
    pub kind: ImplementationKind,
    pub tool: Option<ToolId>,
    pub procedure: Option<ProcedureId>,
    pub verification: Verification,
    pub provenance: Provenance,
    pub dependency_start: u32,
    pub dependency_len: u16,
    pub successes: u32,
    pub failures: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewImplementation {
    pub capability: CapabilityId,
    pub kind: ImplementationKind,
    pub tool: Option<ToolId>,
    pub procedure: Option<ProcedureId>,
    pub verification: Verification,
    pub provenance: Provenance,
}

#[derive(Clone, Debug)]
pub struct CapabilityGraph {
    pub capabilities: Vec<Capability>,
    pub implementations: Vec<Implementation>,
    pub dependencies: Vec<CapabilityId>,
    names: HashMap<String, CapabilityId>,
}

impl Default for CapabilityGraph {
    fn default() -> Self {
        Self::builtins()
    }
}

impl CapabilityGraph {
    pub fn builtins() -> Self {
        let mut graph = Self {
            capabilities: Vec::new(),
            implementations: Vec::new(),
            dependencies: Vec::new(),
            names: HashMap::new(),
        };
        for (id, name, effects, tool) in [
            (
                GIT_STATUS,
                "inspect git status",
                Effects::READ.union(Effects::PROCESS),
                Some(ToolId(1)),
            ),
            (
                GIT_DIFF,
                "inspect git diff",
                Effects::READ.union(Effects::PROCESS),
                Some(ToolId(2)),
            ),
            (
                RUN_TESTS,
                "run tests",
                Effects::READ.union(Effects::PROCESS),
                Some(ToolId(3)),
            ),
            (LIST_FILES, "list files", Effects::READ, Some(ToolId(4))),
            (
                FIND_CHANGED_FILES,
                "find changed files",
                Effects::READ.union(Effects::PROCESS),
                Some(ToolId(5)),
            ),
            (
                INSPECT_INTERFACES,
                "inspect network interfaces",
                Effects::READ,
                Some(ToolId(6)),
            ),
            (
                INSPECT_ROUTES,
                "inspect network routes",
                Effects::READ.union(Effects::PROCESS),
                Some(ToolId(7)),
            ),
            (
                INSPECT_NEIGHBORS,
                "inspect network neighbors",
                Effects::READ.union(Effects::PROCESS),
                Some(ToolId(8)),
            ),
            (
                LIST_SOCKETS,
                "list sockets",
                Effects::READ.union(Effects::PROCESS),
                None,
            ),
            (
                CAPTURE_PACKETS,
                "capture packets",
                Effects::READ.union(Effects::PRIVILEGED),
                None,
            ),
            (TCP_CONNECT, "connect tcp", Effects::NETWORK, None),
            (UDP_EXCHANGE, "exchange udp", Effects::NETWORK, None),
        ] {
            graph.capabilities.push(Capability {
                id,
                name: name.into(),
                effects,
                version: 1,
            });
            graph.names.insert(name.into(), id);
            if let Some(tool) = tool {
                graph.implementations.push(Implementation {
                    capability: id,
                    kind: ImplementationKind::Native,
                    tool: Some(tool),
                    procedure: None,
                    verification: Verification::Verified,
                    provenance: Provenance::BuiltIn,
                    dependency_start: 0,
                    dependency_len: 0,
                    successes: 0,
                    failures: 0,
                });
            }
        }
        graph
    }

    pub fn capability(&self, id: CapabilityId) -> Option<&Capability> {
        self.capabilities
            .get(id.0.checked_sub(1)? as usize)
            .filter(|row| row.id == id)
    }

    pub fn resolve(&self, id: CapabilityId) -> Option<&Implementation> {
        self.resolve_index(id)
            .and_then(|index| self.implementations.get(index))
    }

    pub fn resolve_index(&self, id: CapabilityId) -> Option<usize> {
        self.implementations
            .iter()
            .enumerate()
            .filter(|implementation| {
                implementation.1.capability == id
                    && implementation.1.verification == Verification::Verified
            })
            .min_by_key(|implementation| {
                (
                    implementation.1.kind,
                    std::cmp::Reverse(implementation.1.successes),
                    implementation.1.failures,
                )
            })
            .map(|(index, _)| index)
    }

    pub fn resolve_tool(&self, id: CapabilityId) -> Option<ToolId> {
        self.resolve(id)?.tool
    }

    pub(crate) fn reconcile_builtins(&mut self) -> io::Result<()> {
        for (capability, tool, effects) in [
            (INSPECT_INTERFACES, ToolId(6), Effects::READ),
            (
                INSPECT_ROUTES,
                ToolId(7),
                Effects::READ.union(Effects::PROCESS),
            ),
            (
                INSPECT_NEIGHBORS,
                ToolId(8),
                Effects::READ.union(Effects::PROCESS),
            ),
        ] {
            let row = self
                .capabilities
                .iter_mut()
                .find(|row| row.id == capability)
                .ok_or_else(|| storage::invalid("missing built-in capability"))?;
            row.effects = effects;
            row.version = row.version.max(2);
            if !self.implementations.iter().any(|implementation| {
                implementation.capability == capability
                    && implementation.kind == ImplementationKind::Native
                    && implementation.tool == Some(tool)
                    && implementation.verification == Verification::Verified
            }) {
                self.add_implementation(
                    NewImplementation {
                        capability,
                        kind: ImplementationKind::Native,
                        tool: Some(tool),
                        procedure: None,
                        verification: Verification::Verified,
                        provenance: Provenance::BuiltIn,
                    },
                    &[],
                )?;
            }
        }
        Ok(())
    }

    pub fn add_implementation(
        &mut self,
        draft: NewImplementation,
        dependencies: &[CapabilityId],
    ) -> io::Result<usize> {
        if self.capability(draft.capability).is_none()
            || draft.tool.is_some() == draft.procedure.is_some()
        {
            return Err(storage::invalid("invalid capability implementation"));
        }
        if self.implementations.len() >= MAX_IMPLEMENTATIONS
            || self.dependencies.len() + dependencies.len() > MAX_DEPENDENCIES
            || dependencies.len() > u16::MAX as usize
            || dependencies
                .iter()
                .any(|dependency| self.capability(*dependency).is_none())
        {
            return Err(storage::invalid("capability implementation exceeds bounds"));
        }
        let start = self.dependencies.len() as u32;
        self.dependencies.extend_from_slice(dependencies);
        self.implementations.push(Implementation {
            capability: draft.capability,
            kind: draft.kind,
            tool: draft.tool,
            procedure: draft.procedure,
            verification: draft.verification,
            provenance: draft.provenance,
            dependency_start: start,
            dependency_len: dependencies.len() as u16,
            successes: 0,
            failures: 0,
        });
        Ok(self.implementations.len() - 1)
    }

    pub fn observe(&mut self, index: usize, success: bool) -> io::Result<()> {
        let implementation = self
            .implementations
            .get_mut(index)
            .ok_or_else(|| storage::invalid("unknown capability implementation"))?;
        if success {
            implementation.successes = implementation.successes.saturating_add(1);
        } else {
            implementation.failures = implementation.failures.saturating_add(1);
        }
        Ok(())
    }

    pub(crate) fn encode(&self, writer: &mut impl Write) -> io::Result<()> {
        write_u32(writer, self.capabilities.len() as u32)?;
        for capability in &self.capabilities {
            write_u32(writer, capability.id.0)?;
            write_string(writer, &capability.name)?;
            write_u64(writer, capability.effects.0 as u64)?;
            write_u32(writer, capability.version)?;
        }
        write_u32(writer, self.dependencies.len() as u32)?;
        for dependency in &self.dependencies {
            write_u32(writer, dependency.0)?;
        }
        write_u32(writer, self.implementations.len() as u32)?;
        for implementation in &self.implementations {
            write_u32(writer, implementation.capability.0)?;
            writer.write_all(&[
                implementation.kind as u8,
                implementation.verification as u8,
                implementation.provenance as u8,
            ])?;
            write_u32(writer, implementation.tool.map_or(u32::MAX, |id| id.0))?;
            write_u32(writer, implementation.procedure.map_or(u32::MAX, |id| id.0))?;
            write_u32(writer, implementation.dependency_start)?;
            write_u16(writer, implementation.dependency_len)?;
            write_u32(writer, implementation.successes)?;
            write_u32(writer, implementation.failures)?;
        }
        Ok(())
    }

    pub(crate) fn decode(reader: &mut Reader<'_>) -> io::Result<Self> {
        let count = reader.u32()? as usize;
        if count == 0 || count > MAX_CAPABILITIES {
            return Err(storage::invalid("invalid capability count"));
        }
        let mut graph = Self {
            capabilities: Vec::with_capacity(count),
            implementations: Vec::new(),
            dependencies: Vec::new(),
            names: HashMap::new(),
        };
        for expected in 1..=count {
            let capability = Capability {
                id: CapabilityId(reader.u32()?),
                name: reader.string()?,
                effects: Effects(
                    reader
                        .u64()?
                        .try_into()
                        .map_err(|_| storage::invalid("invalid capability effects"))?,
                ),
                version: reader.u32()?,
            };
            if capability.id.0 != expected as u32
                || capability.name.is_empty()
                || capability.name.len() > 128
                || capability.version == 0
                || capability.effects.0 & !0x7f != 0
                || graph
                    .names
                    .insert(capability.name.clone(), capability.id)
                    .is_some()
            {
                return Err(storage::invalid("invalid capability record"));
            }
            graph.capabilities.push(capability);
        }
        let dependency_count = reader.u32()? as usize;
        if dependency_count > MAX_DEPENDENCIES {
            return Err(storage::invalid("too many capability dependencies"));
        }
        for _ in 0..dependency_count {
            let dependency = CapabilityId(reader.u32()?);
            if graph.capability(dependency).is_none() {
                return Err(storage::invalid("unknown capability dependency"));
            }
            graph.dependencies.push(dependency);
        }
        let implementation_count = reader.u32()? as usize;
        if implementation_count > MAX_IMPLEMENTATIONS {
            return Err(storage::invalid("too many capability implementations"));
        }
        for _ in 0..implementation_count {
            let capability = CapabilityId(reader.u32()?);
            let kind = parse_kind(reader.u8()?)?;
            let verification = parse_verification(reader.u8()?)?;
            let provenance = parse_provenance(reader.u8()?)?;
            let optional = |value| (value != u32::MAX).then_some(value);
            let tool = optional(reader.u32()?).map(ToolId);
            let procedure = optional(reader.u32()?).map(ProcedureId);
            let dependency_start = reader.u32()?;
            let dependency_len = reader.u16()?;
            let implementation = Implementation {
                capability,
                kind,
                tool,
                procedure,
                verification,
                provenance,
                dependency_start,
                dependency_len,
                successes: reader.u32()?,
                failures: reader.u32()?,
            };
            let start = dependency_start as usize;
            if graph.capability(capability).is_none()
                || tool.is_some() == procedure.is_some()
                || start > graph.dependencies.len()
                || dependency_len as usize > graph.dependencies.len() - start
            {
                return Err(storage::invalid("invalid capability implementation"));
            }
            graph.implementations.push(implementation);
        }
        Ok(graph)
    }
}

pub fn for_intent(intent: crate::types::IntentId) -> Option<CapabilityId> {
    Some(match intent.0 {
        1 => GIT_STATUS,
        2 => GIT_DIFF,
        3 => RUN_TESTS,
        4 => LIST_FILES,
        5 => FIND_CHANGED_FILES,
        6 => INSPECT_INTERFACES,
        7 => INSPECT_ROUTES,
        8 => INSPECT_NEIGHBORS,
        _ => return None,
    })
}

fn parse_kind(value: u8) -> io::Result<ImplementationKind> {
    Ok(match value {
        1 => ImplementationKind::Native,
        2 => ImplementationKind::Composition,
        3 => ImplementationKind::System,
        4 => ImplementationKind::Protocol,
        5 => ImplementationKind::Generated,
        6 => ImplementationKind::StructuredExternal,
        7 => ImplementationKind::ApplicationReference,
        8 => ImplementationKind::Accessibility,
        9 => ImplementationKind::Pixels,
        _ => return Err(storage::invalid("unknown implementation kind")),
    })
}

fn parse_verification(value: u8) -> io::Result<Verification> {
    Ok(match value {
        0 => Verification::Unverified,
        1 => Verification::Compiled,
        2 => Verification::Tested,
        3 => Verification::Verified,
        _ => return Err(storage::invalid("unknown verification state")),
    })
}

fn parse_provenance(value: u8) -> io::Result<Provenance> {
    Ok(match value {
        1 => Provenance::BuiltIn,
        2 => Provenance::Configured,
        3 => Provenance::Learned,
        4 => Provenance::Generated,
        5 => Provenance::ApplicationReference,
        _ => return Err(storage::invalid("unknown implementation provenance")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_verified_implementation_beats_application_reference() {
        let mut graph = CapabilityGraph::builtins();
        graph
            .add_implementation(
                NewImplementation {
                    capability: LIST_FILES,
                    kind: ImplementationKind::ApplicationReference,
                    tool: Some(ToolId(99)),
                    procedure: None,
                    verification: Verification::Verified,
                    provenance: Provenance::ApplicationReference,
                },
                &[],
            )
            .unwrap();
        assert_eq!(graph.resolve_tool(LIST_FILES), Some(ToolId(4)));
    }

    #[test]
    fn compilation_alone_does_not_make_generated_code_resolvable() {
        let mut graph = CapabilityGraph::builtins();
        graph
            .add_implementation(
                NewImplementation {
                    capability: LIST_SOCKETS,
                    kind: ImplementationKind::Generated,
                    tool: Some(ToolId(100)),
                    procedure: None,
                    verification: Verification::Compiled,
                    provenance: Provenance::Generated,
                },
                &[],
            )
            .unwrap();
        assert!(graph.resolve(LIST_SOCKETS).is_none());
    }
}
