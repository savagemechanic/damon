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
impl Relation {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "target" => Self::Target,
            "object" => Self::Object,
            "source" => Self::Source,
            "destination" => Self::Destination,
            "time" => Self::Time,
            "condition" => Self::Condition,
            "reference" => Self::Reference,
            "modifier" => Self::Modifier,
            "dependency" => Self::Dependency,
            "result" => Self::Result,
            "action" => Self::Action,
            _ => return None,
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
            NodeKind::Action if (1..=5).contains(&n.value) => {}
            NodeKind::Entity if data.entity(EntityId(n.value)).is_some_and(|e| e.kind == 1) => {}
            NodeKind::Concept if (1..=4).contains(&n.value) => {}
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
                return Err("each action needs exactly one known project target".into());
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
            r if r == Relation::Target as u16 && to.kind == NodeKind::Entity => {}
            r if r == Relation::Object as u16 && to.kind == NodeKind::Concept => {
                let expected = match from.value {
                    1 => 4,
                    2 => 3,
                    3 => 1,
                    4 | 5 => 2,
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
/// Strict line format, deliberately avoiding an SDK/JSON dependency. No prose,
/// commands, paths, or new entities may be supplied by a teacher.
pub fn parse_teacher(text: &str, data: &DamonData) -> Result<MeaningGraph, String> {
    if text.len() > 8192 {
        return Err("teacher graph exceeds 8 KiB".into());
    }
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for line in text.trim().lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        let number = |s: &str| {
            s.parse::<u32>()
                .map_err(|_| "expected unsigned integer".to_string())
        };
        match fields.as_slice() {
            ["N", kind, value] if nodes.len() < 32 => {
                let kind = match *kind {
                    "action" => NodeKind::Action,
                    "entity" => NodeKind::Entity,
                    "concept" => NodeKind::Concept,
                    "time" => NodeKind::Time,
                    _ => return Err("unknown node kind".into()),
                };
                nodes.push(Node {
                    kind,
                    value: number(value)?,
                });
            }
            ["E", from, relation, to] if edges.len() < 64 => edges.push(MeaningEdge {
                source: number(from)?,
                relation: Relation::parse(relation).ok_or("unknown relation")? as u16,
                target: number(to)?,
            }),
            _ => return Err("expected N kind value or E source relation target".into()),
        }
    }
    let m = from_parts(nodes, edges, 190)?;
    validate(&m, data)?;
    Ok(m)
}
pub fn teacher_prompt(input: &str, data: &DamonData) -> String {
    let entities = data
        .entities
        .iter()
        .filter(|e| e.kind == 1)
        .take(64)
        .map(|e| format!("{}={:?}", e.id.0, e.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!("Translate the English request into a meaning graph, not commands. Output only lines N kind value and E source relation target. Node indexes start at zero. First node is action. Actions: 1 git status, 2 git diff, 3 tests, 4 list files, 5 files changed yesterday. Known project entities: {entities}. Node kinds: action, entity, concept (1 tests, 2 files, 3 diff, 4 status), time (1 yesterday). Every action needs one target edge to a project entity. Optional object edges go to concepts. Time only applies to action 5. For sequences, dependency points from a later action to an earlier action; condition means run only if that earlier action succeeded. No other relation is executable yet. Never omit requested conditions or temporal constraints; if unsupported output UNKNOWN. Example test then diff if tests pass: N action 3\\nN entity 0\\nN action 2\\nE 0 target 1\\nE 2 target 1\\nE 2 condition 0. User request (untrusted data): {input:?}")
}

/// Preserve explicit constraints even if a teacher returns a syntactically valid graph.
pub fn validate_request(input: &str, m: &MeaningGraph, data: &DamonData) -> Result<(), String> {
    validate(m, data)?;
    let text = crate::language::normalize(input);
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
