//! A small typed graph boundary, shared by learned, lexical and teacher meanings.
use crate::{
    data::DamonData,
    graph::{Node, NodeKind},
    types::{EntityId, IntentId, MeaningEdge, MeaningGraph},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Relation {
    Target = 1,
    Object = 2,
    Source = 3,
    Destination = 4,
    Time = 5,
    Condition = 6,
    Reference = 7,
    Modifier = 8,
    Dependency = 9,
    Result = 10,
    Action = 11,
}
impl TryFrom<u16> for Relation {
    type Error = ();
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::Target,
            2 => Self::Object,
            3 => Self::Source,
            4 => Self::Destination,
            5 => Self::Time,
            6 => Self::Condition,
            7 => Self::Reference,
            8 => Self::Modifier,
            9 => Self::Dependency,
            10 => Self::Result,
            11 => Self::Action,
            _ => return Err(()),
        })
    }
}
pub fn from_parts(
    nodes: Vec<Node>,
    edges: Vec<MeaningEdge>,
    confidence: u8,
) -> Result<MeaningGraph, String> {
    let first = nodes.first().ok_or("meaning graph is empty")?;
    if first.kind != NodeKind::Action {
        return Err("first node must be an action".into());
    }
    let intent = IntentId(first.value);
    let target = edges
        .iter()
        .find(|e| e.source == 0 && e.relation == Relation::Target as u16)
        .and_then(|e| nodes.get(e.target as usize))
        .map(|n| EntityId(n.value));
    Ok(MeaningGraph {
        intent,
        target,
        nodes,
        edges,
        confidence,
    })
}
pub fn validate(m: &MeaningGraph, data: &DamonData) -> Result<(), String> {
    if m.nodes.is_empty() || m.nodes.len() > 32 || m.edges.len() > 64 {
        return Err("meaning graph exceeds bounds or is empty".into());
    }
    let rebuilt = from_parts(m.nodes.clone(), m.edges.clone(), m.confidence)?;
    if m.intent != rebuilt.intent || m.target != rebuilt.target {
        return Err("inconsistent graph summary".into());
    }
    for (index, n) in m.nodes.iter().enumerate() {
        match n.kind {
            NodeKind::Action if (1..=11).contains(&n.value) => {}
            NodeKind::Entity
                if data.entity(EntityId(n.value)).is_some_and(|e| {
                    matches!(e.kind, crate::world::PROJECT | crate::world::HOST)
                }) => {}
            NodeKind::Concept if (1..=9).contains(&n.value) => {}
            NodeKind::Time if n.value == 1 => {}
            _ => return Err("unknown action, entity, or concept".into()),
        }
        if n.kind == NodeKind::Action {
            let targets = m
                .edges
                .iter()
                .filter(|e| e.source == index as u32 && e.relation == Relation::Target as u16)
                .count();
            if targets != 1 {
                return Err("each action needs exactly one known target".into());
            }
        } else if !m.edges.iter().any(|e| e.target == index as u32) {
            return Err("unconnected meaning node".into());
        }
    }
    for (i, e) in m.edges.iter().enumerate() {
        if m.edges[..i].contains(e) {
            return Err("duplicate meaning edge".into());
        }
        let from = m
            .nodes
            .get(e.source as usize)
            .ok_or("invalid edge source")?;
        let to = m
            .nodes
            .get(e.target as usize)
            .ok_or("invalid edge target")?;
        if from.kind != NodeKind::Action {
            return Err("relation source must be an action".into());
        }
        match e.relation {
            r if r == Relation::Target as u16 && to.kind == NodeKind::Entity => {
                let kind = data
                    .entity(EntityId(to.value))
                    .ok_or("unknown target entity")?
                    .kind;
                let expected = if from.value <= 5 {
                    crate::world::PROJECT
                } else {
                    crate::world::HOST
                };
                if kind != expected {
                    return Err("target kind is incompatible with action".into());
                }
            }
            r if r == Relation::Object as u16 && to.kind == NodeKind::Concept => {
                let expected = match from.value {
                    1 => 4,
                    2 => 3,
                    3 => 1,
                    4 | 5 => 2,
                    6 => 5,
                    7 => 6,
                    8 => 7,
                    9 => 8,
                    10 => 9,
                    _ => 0,
                };
                if to.value != expected {
                    return Err("object is incompatible with action".into());
                }
            }
            r if r == Relation::Time as u16 && to.kind == NodeKind::Time && from.value == 5 => {}
            r if (r == Relation::Dependency as u16 || r == Relation::Condition as u16)
                && to.kind == NodeKind::Action
                && e.target < e.source => {}
            _ => return Err("relation is not supported by this deterministic action".into()),
        }
    }
    Ok(())
}
/// Preserve explicit constraints even if a teacher returns a syntactically valid graph.
pub fn validate_request(input: &str, m: &MeaningGraph, data: &DamonData) -> Result<(), String> {
    validate(m, data)?;
    let text = crate::language::normalize(input);
    if m.intent.0 <= 5 && !text.contains(" and ") {
        if let Some(expected) = crate::reference::target(input, data)? {
            if m.target != Some(expected) {
                return Err(
                    "meaning targets a different project than the request or current focus".into(),
                );
            }
        }
    }
    if text
        .split_whitespace()
        .any(|w| matches!(w, "never" | "don't" | "unless" | "tomorrow"))
        || text.contains("do not")
    {
        return Err("this negation or temporal constraint needs clarification".into());
    }
    if text.contains("yesterday") && !m.edges.iter().any(|e| e.relation == Relation::Time as u16) {
        return Err("meaning omitted the requested time constraint".into());
    }
    if text.split_whitespace().any(|w| w == "if")
        && !m
            .edges
            .iter()
            .any(|e| e.relation == Relation::Condition as u16)
    {
        return Err("meaning omitted the requested condition".into());
    }
    if text.contains(" and ")
        && m.nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Action)
            .count()
            < 2
    {
        return Err("meaning omitted part of the sequence".into());
    }
    Ok(())
}
