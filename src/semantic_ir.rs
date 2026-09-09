//! The model-independent, meaning-only boundary between language and execution.
use crate::{data::DamonData, semantic_registry as registry, types::ConceptId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const IR_VERSION: u16 = 1;
pub const MAX_NODES: usize = 32;
pub const MAX_EDGES: usize = 64;
pub const MAX_CANDIDATES: usize = 3;
pub const MAX_SLOTS: usize = 64;
pub const MAX_INPUT_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NodeKind {
    Action,
    Entity,
    Value,
    Constraint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub start: u16,
    pub end: u16,
    pub expected_kind: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub kind: NodeKind,
    pub concept: u32,
    pub value: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub source: u16,
    pub predicate: u32,
    pub target: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateIr {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntitySlot {
    pub slot: u16,
    pub entity: crate::types::EntityId,
    pub kind: ConceptId,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct SemanticRequest {
    pub input: String,
    pub slots: Vec<EntitySlot>,
    pub allowed_actions: Vec<ConceptId>,
    pub allowed_predicates: Vec<ConceptId>,
    pub allowed_entity_kinds: Vec<ConceptId>,
    pub ir_version: u16,
    pub registry_version: u16,
    pub previous_action: Option<ConceptId>,
    pub focus_slot: Option<u16>,
    pub action_priors: Vec<(ConceptId, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityBinding {
    Persistent(crate::types::EntityId),
    SourceSpan(SourceSpan),
    Abstract,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundSemanticIr {
    pub ir: CandidateIr,
    pub entities: Vec<(u16, EntityBinding)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolutionStatus {
    Resolved,
    Ambiguous,
    Unknown,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProducerKind {
    Deterministic,
    Learned,
    Model,
}

#[derive(Clone, Debug)]
pub struct ScoredCandidate {
    pub ir: CandidateIr,
    pub evidence: i32,
}

#[derive(Clone, Debug)]
pub struct SemanticResolution {
    pub status: ResolutionStatus,
    pub candidates: Vec<ScoredCandidate>,
    pub unresolved_spans: Vec<SourceSpan>,
    pub producer: ProducerKind,
    pub diagnostic: Option<String>,
}

pub trait SemanticProducer {
    fn resolve(&self, request: &SemanticRequest) -> SemanticResolution;
}

pub struct JsonSemanticProducer<'a> {
    pub output: &'a str,
}

impl SemanticProducer for JsonSemanticProducer<'_> {
    fn resolve(&self, request: &SemanticRequest) -> SemanticResolution {
        parse_json(self.output, request).unwrap_or_else(|diagnostic| SemanticResolution {
            status: ResolutionStatus::Invalid,
            candidates: Vec::new(),
            unresolved_spans: Vec::new(),
            producer: ProducerKind::Model,
            diagnostic: Some(diagnostic),
        })
    }
}

pub fn from_meaning(
    meaning: &crate::types::MeaningGraph,
    request: &SemanticRequest,
) -> Result<CandidateIr, String> {
    let mut nodes = Vec::new();
    let mut indexes = vec![None; meaning.nodes.len()];
    for (old_index, node) in meaning.nodes.iter().enumerate() {
        let converted = match node.kind {
            crate::graph::NodeKind::Action => Node {
                kind: NodeKind::Action,
                concept: registry::intent_to_action(crate::types::IntentId(node.value))
                    .ok_or("unknown action")?
                    .0,
                value: 0,
                span: None,
            },
            crate::graph::NodeKind::Entity => {
                let entity = crate::types::EntityId(node.value);
                let slot = request
                    .slots
                    .iter()
                    .find(|slot| slot.entity == entity)
                    .ok_or("bound entity was not exposed as a context slot")?;
                Node {
                    kind: NodeKind::Entity,
                    concept: slot.kind.0,
                    value: u32::from(slot.slot),
                    span: None,
                }
            }
            crate::graph::NodeKind::Time => Node {
                kind: NodeKind::Value,
                concept: registry::YESTERDAY.0,
                value: node.value,
                span: None,
            },
            crate::graph::NodeKind::Concept | crate::graph::NodeKind::Condition => continue,
        };
        indexes[old_index] = Some(nodes.len() as u16);
        nodes.push(converted);
    }
    let mut edges = Vec::new();
    for edge in &meaning.edges {
        let relation = crate::semantics::Relation::try_from(edge.relation).ok();
        let (source, predicate, target) = match relation {
            Some(crate::semantics::Relation::Target) => (
                indexes[edge.source as usize],
                registry::TARGET,
                indexes[edge.target as usize],
            ),
            Some(crate::semantics::Relation::Time) => (
                indexes[edge.source as usize],
                registry::TIME,
                indexes[edge.target as usize],
            ),
            Some(crate::semantics::Relation::Dependency) => (
                indexes[edge.target as usize],
                registry::AFTER,
                indexes[edge.source as usize],
            ),
            Some(crate::semantics::Relation::Condition) => (
                indexes[edge.target as usize],
                registry::ON_SUCCESS,
                indexes[edge.source as usize],
            ),
            _ => continue,
        };
        if let (Some(source), Some(target)) = (source, target) {
            edges.push(Edge {
                source,
                predicate: predicate.0,
                target,
            });
        }
    }
    Ok(CandidateIr { nodes, edges })
}

pub fn bind(
    ir: &CandidateIr,
    request: &SemanticRequest,
    data: &DamonData,
    confidence: u8,
) -> Result<crate::types::MeaningGraph, String> {
    let bound = bind_context(ir, request, data)?;
    let mut nodes = Vec::new();
    let mut indexes = vec![None; ir.nodes.len()];
    for (index, node) in ir.nodes.iter().enumerate() {
        let converted = match node.kind {
            NodeKind::Action => crate::graph::Node {
                kind: crate::graph::NodeKind::Action,
                value: registry::action_to_intent(ConceptId(node.concept))
                    .ok_or("unknown action")?
                    .0,
            },
            NodeKind::Entity => {
                let EntityBinding::Persistent(entity) = bound
                    .entities
                    .iter()
                    .find(|(node_index, _)| usize::from(*node_index) == index)
                    .map(|(_, binding)| *binding)
                    .ok_or("entity was not bound")?
                else {
                    return Err("this valid entity mention has no executable lowering yet".into());
                };
                crate::graph::Node {
                    kind: crate::graph::NodeKind::Entity,
                    value: entity.0,
                }
            }
            NodeKind::Value if node.concept == registry::YESTERDAY.0 => crate::graph::Node {
                kind: crate::graph::NodeKind::Time,
                value: node.value,
            },
            NodeKind::Value | NodeKind::Constraint => {
                return Err("this valid meaning has no executable lowering yet".into())
            }
        };
        indexes[index] = Some(nodes.len() as u32);
        nodes.push(converted);
    }
    let mut edges = Vec::new();
    for edge in &ir.edges {
        let (source, relation, target) = match ConceptId(edge.predicate) {
            registry::TARGET => (edge.source, crate::semantics::Relation::Target, edge.target),
            registry::TIME => (edge.source, crate::semantics::Relation::Time, edge.target),
            registry::AFTER => (
                edge.target,
                crate::semantics::Relation::Dependency,
                edge.source,
            ),
            registry::ON_SUCCESS => (
                edge.target,
                crate::semantics::Relation::Condition,
                edge.source,
            ),
            _ => continue,
        };
        edges.push(crate::types::MeaningEdge {
            source: indexes[source as usize].ok_or("edge source was not lowerable")?,
            relation: relation as u16,
            target: indexes[target as usize].ok_or("edge target was not lowerable")?,
        });
    }
    // Preserve the compact object concepts used by the current execution graph.
    let action_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            (node.kind == crate::graph::NodeKind::Action).then_some((index, node.value))
        })
        .collect::<Vec<_>>();
    for (index, action) in action_nodes {
        let object = match action {
            1 => 4,
            2 => 3,
            3 => 1,
            4 | 5 => 2,
            6 => 5,
            7 => 6,
            8 => 7,
            _ => continue,
        };
        let target = nodes.len() as u32;
        nodes.push(crate::graph::Node {
            kind: crate::graph::NodeKind::Concept,
            value: object,
        });
        edges.push(crate::types::MeaningEdge {
            source: index as u32,
            relation: crate::semantics::Relation::Object as u16,
            target,
        });
    }
    let meaning = crate::semantics::from_parts(nodes, edges, confidence)?;
    crate::semantics::validate(&meaning, data)?;
    Ok(meaning)
}

pub fn bind_context(
    ir: &CandidateIr,
    request: &SemanticRequest,
    data: &DamonData,
) -> Result<BoundSemanticIr, String> {
    validate_structural(ir, &request.input)?;
    validate_semantic(ir, request)?;
    let mut entities = Vec::new();
    for (index, node) in ir.nodes.iter().enumerate() {
        if node.kind != NodeKind::Entity {
            continue;
        }
        let binding = if let Some(span) = node.span {
            EntityBinding::SourceSpan(span)
        } else if node.concept == registry::FILE_SET.0 {
            EntityBinding::Abstract
        } else {
            let slot = request
                .slots
                .iter()
                .find(|slot| u32::from(slot.slot) == node.value)
                .ok_or("entity slot is not exposed")?;
            if data.entity(slot.entity).is_none() {
                return Err("entity slot no longer resolves".into());
            }
            EntityBinding::Persistent(slot.entity)
        };
        entities.push((
            index
                .try_into()
                .map_err(|_| "semantic node index exceeds 16 bits")?,
            binding,
        ));
    }
    Ok(BoundSemanticIr {
        ir: ir.clone(),
        entities,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireResolution {
    ir_version: u16,
    registry_version: u16,
    candidates: Vec<CandidateIr>,
    #[serde(default)]
    unresolved_spans: Vec<SourceSpan>,
}

pub fn request(input: &str, data: &DamonData) -> SemanticRequest {
    let text = crate::language::normalize(input);
    let repeated_meaning = (text.contains("same thing")
        || matches!(text.as_str(), "do that again" | "repeat that" | "again"))
    .then(|| {
        data.world
            .context
            .previous_feature
            .and_then(|feature| data.learned_graphs.get(&feature))
    })
    .flatten();
    let mut ids = Vec::new();
    for id in crate::reference::project_mentions(input, data)
        .into_iter()
        .chain(data.world.context.focus)
        .chain(data.resolve("damon"))
        .chain(data.resolve("local host"))
        .chain(
            crate::language::normalize(input)
                .contains("desktop")
                .then(|| data.resolve("desktop"))
                .flatten(),
        )
    {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids.truncate(MAX_SLOTS);
    let slots = ids
        .into_iter()
        .enumerate()
        .filter_map(|(slot, id)| {
            let entity = data.entity(id)?;
            Some(EntitySlot {
                slot: slot as u16,
                entity: id,
                kind: registry::entity_kind(entity.kind)?,
                name: entity.name.clone(),
            })
        })
        .collect::<Vec<_>>();
    let mut allowed_actions = Vec::new();
    let relevant = [
        (registry::RUN_TESTS, ["test", "check"]),
        (registry::SHOW_DIFF, ["diff", "changed"]),
        (registry::GIT_STATUS, ["status", "repo"]),
        (registry::LIST_FILES, ["files", "list"]),
        (registry::FIND_CHANGED_FILES, ["yesterday", "modified"]),
        (registry::INSPECT_INTERFACES, ["network", "connected"]),
        (registry::INSPECT_ROUTES, ["gateway", "traffic"]),
        (registry::INSPECT_NEIGHBORS, ["devices", "neighbors"]),
        (registry::COPY, ["copy", "duplicate"]),
        (registry::FIND, ["find", "larger"]),
    ];
    for (action, cues) in relevant {
        if cues.iter().any(|cue| text.contains(cue)) {
            allowed_actions.push(action);
        }
    }
    if text.contains("inspect") {
        allowed_actions.push(registry::LIST_FILES);
    }
    for (intent, _) in data.language_candidates(crate::language::feature_hash(input)) {
        if let Some(action) = registry::intent_to_action(*intent) {
            allowed_actions.push(action);
        }
    }
    if let Some(meaning) = data
        .learned_graphs
        .get(&crate::language::feature_hash(input))
        .or(repeated_meaning)
    {
        for node in &meaning.nodes {
            if node.kind == crate::graph::NodeKind::Action {
                if let Some(action) = registry::intent_to_action(crate::types::IntentId(node.value))
                {
                    allowed_actions.push(action);
                }
            }
        }
    }
    if let Some(action) = data
        .world
        .context
        .previous_action
        .and_then(registry::intent_to_action)
    {
        allowed_actions.push(action);
    }
    if let Some(meaning) = data
        .world
        .context
        .previous_feature
        .and_then(|feature| data.learned_graphs.get(&feature))
    {
        for node in &meaning.nodes {
            if node.kind == crate::graph::NodeKind::Action {
                if let Some(action) = registry::intent_to_action(crate::types::IntentId(node.value))
                {
                    allowed_actions.push(action);
                }
            }
        }
    }
    allowed_actions.sort();
    allowed_actions.dedup();
    allowed_actions.truncate(8);
    let mut allowed_predicates = vec![registry::TARGET];
    if allowed_actions.contains(&registry::COPY) {
        allowed_predicates.extend([registry::OBJECT, registry::SOURCE, registry::DESTINATION]);
    }
    if allowed_actions.contains(&registry::FIND) {
        allowed_predicates.extend([registry::OBJECT, registry::REQUIRES, registry::VALUE]);
    }
    if allowed_actions.contains(&registry::FIND_CHANGED_FILES) {
        allowed_predicates.push(registry::TIME);
    }
    if text.contains(" and ")
        || text.contains(" then ")
        || text.contains(" if ")
        || data
            .learned_graphs
            .get(&crate::language::feature_hash(input))
            .or(repeated_meaning)
            .is_some_and(|meaning| {
                meaning
                    .nodes
                    .iter()
                    .filter(|node| node.kind == crate::graph::NodeKind::Action)
                    .count()
                    > 1
            })
        || data
            .world
            .context
            .previous_feature
            .and_then(|feature| data.learned_graphs.get(&feature))
            .is_some_and(|meaning| {
                meaning
                    .nodes
                    .iter()
                    .filter(|node| node.kind == crate::graph::NodeKind::Action)
                    .count()
                    > 1
            })
    {
        allowed_predicates.extend([registry::AFTER, registry::ON_SUCCESS, registry::ON_FAILURE]);
    }
    allowed_predicates.sort();
    allowed_predicates.dedup();
    let mut allowed_entity_kinds = slots.iter().map(|slot| slot.kind).collect::<Vec<_>>();
    if allowed_actions.contains(&registry::COPY) {
        allowed_entity_kinds.push(registry::FILE);
    }
    if allowed_actions.contains(&registry::FIND) {
        allowed_entity_kinds.push(registry::FILE_SET);
    }
    allowed_entity_kinds.sort();
    allowed_entity_kinds.dedup();
    let focus_slot = data.world.context.focus.and_then(|focus| {
        slots
            .iter()
            .find(|slot| slot.entity == focus)
            .map(|slot| slot.slot)
    });
    SemanticRequest {
        input: truncate_utf8(input, MAX_INPUT_BYTES).to_string(),
        slots,
        allowed_actions,
        allowed_predicates,
        allowed_entity_kinds,
        ir_version: IR_VERSION,
        registry_version: registry::VERSION,
        previous_action: data
            .world
            .context
            .previous_action
            .and_then(registry::intent_to_action),
        focus_slot,
        action_priors: data
            .language_candidates(crate::language::feature_hash(input))
            .iter()
            .filter_map(|(intent, count)| {
                registry::intent_to_action(*intent).map(|action| (action, *count))
            })
            .take(8)
            .collect(),
    }
}

fn truncate_utf8(input: &str, maximum: usize) -> &str {
    if input.len() <= maximum {
        return input;
    }
    let mut end = maximum;
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    &input[..end]
}

pub fn parse_json(text: &str, request: &SemanticRequest) -> Result<SemanticResolution, String> {
    if text.len() > 32 * 1024 {
        return Err("semantic JSON exceeds 32 KiB".into());
    }
    let wire: WireResolution =
        serde_json::from_str(text).map_err(|error| format!("invalid semantic JSON: {error}"))?;
    if wire.ir_version != request.ir_version || wire.registry_version != request.registry_version {
        return Err("semantic or registry version mismatch".into());
    }
    if wire.candidates.len() > MAX_CANDIDATES || wire.unresolved_spans.len() > MAX_NODES {
        return Err("semantic candidate or unresolved-span limit exceeded".into());
    }
    for span in &wire.unresolved_spans {
        validate_span(span, &request.input)?;
        if !request
            .allowed_entity_kinds
            .contains(&ConceptId(span.expected_kind))
        {
            return Err("unresolved span kind was not exposed".into());
        }
    }
    let mut candidates = Vec::new();
    for candidate in wire.candidates {
        validate_structural(&candidate, &request.input)?;
        validate_semantic(&candidate, request)?;
        let evidence = score_candidate(&candidate, request);
        candidates.push(ScoredCandidate {
            ir: candidate,
            evidence,
        });
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.evidence));
    let status = match candidates.as_slice() {
        [] => ResolutionStatus::Unknown,
        [_] => ResolutionStatus::Resolved,
        [first, second, ..] if first.evidence - second.evidence >= 24 => ResolutionStatus::Resolved,
        _ => ResolutionStatus::Ambiguous,
    };
    Ok(SemanticResolution {
        status,
        candidates,
        unresolved_spans: wire.unresolved_spans,
        producer: ProducerKind::Model,
        diagnostic: None,
    })
}

fn score_candidate(ir: &CandidateIr, request: &SemanticRequest) -> i32 {
    let mut score = 64;
    for node in &ir.nodes {
        match node.kind {
            NodeKind::Action => {
                let action = registry::action_to_intent(ConceptId(node.concept));
                if action == request.previous_action.and_then(registry::action_to_intent) {
                    score += 24;
                }
                if let Some((_, count)) = request
                    .action_priors
                    .iter()
                    .find(|(id, _)| id.0 == node.concept)
                {
                    score += integer_log2(count.saturating_add(1)) as i32 * 8;
                }
            }
            NodeKind::Entity if node.span.is_none() => score += 16,
            NodeKind::Entity => score += 8,
            NodeKind::Value | NodeKind::Constraint => score += 4,
        }
    }
    score
}

fn integer_log2(value: u32) -> u32 {
    31 - value.max(1).leading_zeros()
}

pub fn computed_confidence(resolution: &SemanticResolution, index: usize) -> u8 {
    let Some(candidate) = resolution.candidates.get(index) else {
        return 0;
    };
    let next = resolution
        .candidates
        .get(index + 1)
        .map_or(candidate.evidence - 32, |other| other.evidence);
    let margin = (candidate.evidence - next).clamp(0, 63) as u8;
    let validity: u8 = if resolution.status == ResolutionStatus::Invalid {
        0
    } else {
        128
    };
    validity.saturating_add(margin)
}

pub fn validate_structural(ir: &CandidateIr, input: &str) -> Result<(), String> {
    if ir.nodes.is_empty() || ir.nodes.len() > MAX_NODES || ir.edges.len() > MAX_EDGES {
        return Err("semantic graph exceeds bounds or is empty".into());
    }
    if ir
        .nodes
        .first()
        .is_none_or(|node| node.kind != NodeKind::Action)
    {
        return Err("first semantic node must be an action".into());
    }
    for node in &ir.nodes {
        if let Some(span) = &node.span {
            validate_span(span, input)?;
        }
    }
    let mut seen = HashSet::new();
    for edge in &ir.edges {
        if edge.source as usize >= ir.nodes.len() || edge.target as usize >= ir.nodes.len() {
            return Err("semantic edge index is out of bounds".into());
        }
        if edge.source == edge.target || !seen.insert((edge.source, edge.predicate, edge.target)) {
            return Err("semantic graph contains a self-edge or duplicate edge".into());
        }
    }
    Ok(())
}

fn validate_span(span: &SourceSpan, input: &str) -> Result<(), String> {
    let start = usize::from(span.start);
    let end = usize::from(span.end);
    if start >= end
        || end > input.len()
        || end - start > 256
        || !input.is_char_boundary(start)
        || !input.is_char_boundary(end)
        || registry::namespace(ConceptId(span.expected_kind)) != registry::ENTITY_KIND_NAMESPACE
        || !registry::known(ConceptId(span.expected_kind))
    {
        return Err("invalid source span".into());
    }
    Ok(())
}

pub fn validate_semantic(ir: &CandidateIr, request: &SemanticRequest) -> Result<(), String> {
    let actions = request
        .allowed_actions
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let predicates = request
        .allowed_predicates
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let kinds = request
        .allowed_entity_kinds
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    for node in &ir.nodes {
        let concept = ConceptId(node.concept);
        if !registry::known(concept) {
            return Err("unknown semantic concept ID".into());
        }
        match node.kind {
            NodeKind::Action if actions.contains(&concept) => {}
            NodeKind::Entity if kinds.contains(&concept) => {
                if node
                    .span
                    .is_some_and(|span| span.expected_kind != node.concept)
                {
                    return Err("source span kind does not match entity kind".into());
                }
                if node.span.is_some() && node.value != 0 {
                    return Err("source-span entities cannot also carry a slot".into());
                }
                if node.span.is_none()
                    && concept != registry::FILE_SET
                    && !request
                        .slots
                        .iter()
                        .any(|slot| u32::from(slot.slot) == node.value && slot.kind == concept)
                {
                    return Err("entity slot was not exposed or has the wrong kind".into());
                }
            }
            NodeKind::Value if registry::namespace(concept) == registry::VALUE_TYPE_NAMESPACE => {}
            NodeKind::Constraint
                if registry::namespace(concept) == registry::PROPERTY_NAMESPACE
                    && registry::namespace(ConceptId(node.value))
                        == registry::OPERATOR_NAMESPACE
                    && registry::known(ConceptId(node.value)) => {}
            _ => return Err("node concept is incompatible with its kind or exposure".into()),
        }
    }
    for edge in &ir.edges {
        let predicate = ConceptId(edge.predicate);
        if !predicates.contains(&predicate) || !registry::known(predicate) {
            return Err("predicate was not exposed".into());
        }
        let source = &ir.nodes[edge.source as usize];
        let target = &ir.nodes[edge.target as usize];
        match predicate {
            registry::TARGET | registry::OBJECT | registry::SOURCE | registry::DESTINATION
                if source.kind == NodeKind::Action && target.kind == NodeKind::Entity => {}
            registry::TIME if source.kind == NodeKind::Action && target.kind == NodeKind::Value => {
            }
            registry::AFTER | registry::ON_SUCCESS | registry::ON_FAILURE | registry::REQUIRES
                if source.kind == NodeKind::Action
                    && matches!(target.kind, NodeKind::Action | NodeKind::Constraint) => {}
            registry::VALUE
                if source.kind == NodeKind::Constraint && target.kind == NodeKind::Value => {}
            _ => return Err("predicate is illegal for these node kinds".into()),
        }
    }
    validate_requirements(ir)
}

fn validate_requirements(ir: &CandidateIr) -> Result<(), String> {
    for (index, node) in ir.nodes.iter().enumerate().skip(1) {
        if node.kind != NodeKind::Action
            && !ir
                .edges
                .iter()
                .any(|edge| edge.source as usize == index || edge.target as usize == index)
        {
            return Err("semantic graph contains an unconnected node".into());
        }
    }
    let action_count = ir
        .nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Action)
        .count();
    if action_count > 1 {
        for (index, node) in ir.nodes.iter().enumerate().skip(1) {
            if node.kind == NodeKind::Action
                && !ir.edges.iter().any(|edge| {
                    edge.target as usize == index
                        && matches!(
                            ConceptId(edge.predicate),
                            registry::AFTER
                                | registry::ON_SUCCESS
                                | registry::ON_FAILURE
                                | registry::REQUIRES
                        )
                })
            {
                return Err("multiple actions must form one composition DAG".into());
            }
        }
    }
    for (index, node) in ir.nodes.iter().enumerate() {
        if node.kind != NodeKind::Action {
            continue;
        }
        let has = |predicate: ConceptId| {
            ir.edges
                .iter()
                .any(|edge| edge.source as usize == index && edge.predicate == predicate.0)
        };
        let count = |predicate: ConceptId| {
            ir.edges
                .iter()
                .filter(|edge| edge.source as usize == index && edge.predicate == predicate.0)
                .count()
        };
        let target_kind = |predicate: ConceptId| {
            ir.edges
                .iter()
                .find(|edge| edge.source as usize == index && edge.predicate == predicate.0)
                .map(|edge| ConceptId(ir.nodes[edge.target as usize].concept))
        };
        match ConceptId(node.concept) {
            registry::COPY
                if count(registry::OBJECT) != 1
                    || count(registry::DESTINATION) != 1
                    || count(registry::SOURCE) > 1 =>
            {
                return Err("COPY requires OBJECT and DESTINATION".into())
            }
            registry::COPY
                if target_kind(registry::OBJECT) != Some(registry::FILE)
                    || target_kind(registry::DESTINATION) != Some(registry::DIRECTORY)
                    || (has(registry::SOURCE)
                        && !matches!(
                            target_kind(registry::SOURCE),
                            Some(registry::PROJECT) | Some(registry::DIRECTORY)
                        )) =>
            {
                return Err("COPY arguments have incompatible entity kinds".into())
            }
            registry::FIND if count(registry::OBJECT) != 1 => {
                return Err("FIND requires OBJECT".into())
            }
            registry::FIND if target_kind(registry::OBJECT) != Some(registry::FILE_SET) => {
                return Err("FIND object must be a file set".into())
            }
            action
                if registry::namespace(action) == registry::ACTION_NAMESPACE
                    && !matches!(action, registry::COPY | registry::FIND)
                    && count(registry::TARGET) != 1 =>
            {
                return Err("action requires TARGET".into())
            }
            action
                if matches!(
                    action,
                    registry::GIT_STATUS
                        | registry::SHOW_DIFF
                        | registry::RUN_TESTS
                        | registry::LIST_FILES
                        | registry::FIND_CHANGED_FILES
                ) && target_kind(registry::TARGET) != Some(registry::PROJECT) =>
            {
                return Err("coding action target must be a project".into())
            }
            action
                if matches!(
                    action,
                    registry::INSPECT_INTERFACES
                        | registry::INSPECT_ROUTES
                        | registry::INSPECT_NEIGHBORS
                ) && target_kind(registry::TARGET) != Some(registry::HOST) =>
            {
                return Err("network action target must be a host".into())
            }
            _ => {}
        }
    }
    for (index, node) in ir.nodes.iter().enumerate() {
        if node.kind != NodeKind::Constraint {
            continue;
        }
        let values = ir
            .edges
            .iter()
            .filter(|edge| edge.source as usize == index && edge.predicate == registry::VALUE.0)
            .collect::<Vec<_>>();
        if values.len() != 1 {
            return Err("constraint requires exactly one typed value".into());
        }
        let value = &ir.nodes[values[0].target as usize];
        if ConceptId(node.concept) == registry::SIZE && ConceptId(value.concept) != registry::BYTES
        {
            return Err("SIZE constraint requires a byte value".into());
        }
    }
    validate_dag(ir)
}

fn validate_dag(ir: &CandidateIr) -> Result<(), String> {
    let mut incoming = vec![0_u8; ir.nodes.len()];
    let composition = |id| {
        matches!(
            ConceptId(id),
            registry::AFTER | registry::ON_SUCCESS | registry::ON_FAILURE | registry::REQUIRES
        )
    };
    for edge in &ir.edges {
        if composition(edge.predicate) && ir.nodes[edge.target as usize].kind == NodeKind::Action {
            incoming[edge.target as usize] = incoming[edge.target as usize].saturating_add(1);
        }
    }
    let mut queue = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(node) = queue.pop() {
        visited += 1;
        for edge in ir.edges.iter().filter(|edge| {
            edge.source as usize == node
                && composition(edge.predicate)
                && ir.nodes[edge.target as usize].kind == NodeKind::Action
        }) {
            let target = edge.target as usize;
            incoming[target] -= 1;
            if incoming[target] == 0 {
                queue.push(target);
            }
        }
    }
    if visited < ir.nodes.len() {
        return Err("semantic composition must be acyclic".into());
    }
    Ok(())
}

pub fn canonical_bytes(ir: &CandidateIr) -> Result<Vec<u8>, String> {
    validate_shape(ir)?;
    let mut out = Vec::new();
    cbor_array(&mut out, 4);
    cbor_uint(&mut out, u64::from(IR_VERSION));
    cbor_uint(&mut out, u64::from(registry::VERSION));
    cbor_array(&mut out, ir.nodes.len() as u64);
    for node in &ir.nodes {
        cbor_array(&mut out, 4);
        cbor_uint(&mut out, node.kind as u64);
        cbor_uint(&mut out, u64::from(node.concept));
        cbor_uint(&mut out, u64::from(node.value));
        match node.span {
            None => out.push(0xf6),
            Some(span) => {
                cbor_array(&mut out, 3);
                cbor_uint(&mut out, u64::from(span.start));
                cbor_uint(&mut out, u64::from(span.end));
                cbor_uint(&mut out, u64::from(span.expected_kind));
            }
        }
    }
    cbor_array(&mut out, ir.edges.len() as u64);
    for edge in &ir.edges {
        cbor_array(&mut out, 3);
        cbor_uint(&mut out, u64::from(edge.source));
        cbor_uint(&mut out, u64::from(edge.predicate));
        cbor_uint(&mut out, u64::from(edge.target));
    }
    Ok(out)
}

pub fn canonical_hash(ir: &CandidateIr) -> Result<u64, String> {
    let bytes = canonical_bytes(ir)?;
    Ok(crate::cache::hash_bytes(&[&bytes]))
}

fn validate_shape(ir: &CandidateIr) -> Result<(), String> {
    if ir.nodes.is_empty() || ir.nodes.len() > MAX_NODES || ir.edges.len() > MAX_EDGES {
        return Err("semantic graph exceeds bounds or is empty".into());
    }
    if ir
        .nodes
        .first()
        .is_none_or(|node| node.kind != NodeKind::Action)
    {
        return Err("first semantic node must be an action".into());
    }
    let mut seen = HashSet::new();
    for edge in &ir.edges {
        if edge.source as usize >= ir.nodes.len() || edge.target as usize >= ir.nodes.len() {
            return Err("semantic edge index is out of bounds".into());
        }
        if edge.source == edge.target || !seen.insert((edge.source, edge.predicate, edge.target)) {
            return Err("semantic graph contains a self-edge or duplicate edge".into());
        }
    }
    Ok(())
}

fn cbor_array(out: &mut Vec<u8>, len: u64) {
    cbor_major(out, 4, len);
}

fn cbor_uint(out: &mut Vec<u8>, value: u64) {
    cbor_major(out, 0, value);
}

fn cbor_major(out: &mut Vec<u8>, major: u8, value: u64) {
    let head = major << 5;
    match value {
        0..=23 => out.push(head | value as u8),
        24..=0xff => out.extend([head | 24, value as u8]),
        0x100..=0xffff => {
            out.push(head | 25);
            out.extend((value as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            out.push(head | 26);
            out.extend((value as u32).to_be_bytes());
        }
        _ => {
            out.push(head | 27);
            out.extend(value.to_be_bytes());
        }
    }
}

pub fn decode_canonical(bytes: &[u8], request: &SemanticRequest) -> Result<CandidateIr, String> {
    if bytes.len() > 32 * 1024 {
        return Err("canonical semantic IR exceeds 32 KiB".into());
    }
    let mut reader = CborReader { bytes, position: 0 };
    reader.array(4)?;
    if reader.uint()? != u64::from(IR_VERSION) || reader.uint()? != u64::from(registry::VERSION) {
        return Err("semantic or registry version mismatch".into());
    }
    let node_count = reader.array_any()?;
    if node_count == 0 || node_count > MAX_NODES {
        return Err("semantic node limit exceeded".into());
    }
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        reader.array(4)?;
        let kind = match reader.uint()? {
            0 => NodeKind::Action,
            1 => NodeKind::Entity,
            2 => NodeKind::Value,
            3 => NodeKind::Constraint,
            _ => return Err("unknown semantic node kind".into()),
        };
        let concept = reader.u32()?;
        let value = reader.u32()?;
        let span = if reader.is_null() {
            reader.null()?;
            None
        } else {
            reader.array(3)?;
            Some(SourceSpan {
                start: reader.u16()?,
                end: reader.u16()?,
                expected_kind: reader.u32()?,
            })
        };
        nodes.push(Node {
            kind,
            concept,
            value,
            span,
        });
    }
    let edge_count = reader.array_any()?;
    if edge_count > MAX_EDGES {
        return Err("semantic edge limit exceeded".into());
    }
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        reader.array(3)?;
        edges.push(Edge {
            source: reader.u16()?,
            predicate: reader.u32()?,
            target: reader.u16()?,
        });
    }
    if reader.position != bytes.len() {
        return Err("trailing canonical semantic bytes".into());
    }
    let ir = CandidateIr { nodes, edges };
    validate_structural(&ir, &request.input)?;
    validate_semantic(&ir, request)?;
    Ok(ir)
}

struct CborReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl CborReader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let byte = *self
            .bytes
            .get(self.position)
            .ok_or("truncated canonical semantic IR")?;
        self.position += 1;
        Ok(byte)
    }

    fn uint(&mut self) -> Result<u64, String> {
        let (major, value) = self.item()?;
        if major != 0 {
            return Err("expected canonical unsigned integer".into());
        }
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32, String> {
        self.uint()?
            .try_into()
            .map_err(|_| "semantic integer exceeds 32 bits".into())
    }

    fn u16(&mut self) -> Result<u16, String> {
        self.uint()?
            .try_into()
            .map_err(|_| "semantic integer exceeds 16 bits".into())
    }

    fn array(&mut self, expected: usize) -> Result<(), String> {
        if self.array_any()? != expected {
            return Err("unexpected canonical array length".into());
        }
        Ok(())
    }

    fn array_any(&mut self) -> Result<usize, String> {
        let (major, value) = self.item()?;
        if major != 4 {
            return Err("expected canonical array".into());
        }
        value
            .try_into()
            .map_err(|_| "canonical array length is too large".into())
    }

    fn is_null(&self) -> bool {
        self.bytes.get(self.position) == Some(&0xf6)
    }

    fn null(&mut self) -> Result<(), String> {
        if self.byte()? != 0xf6 {
            return Err("expected canonical null".into());
        }
        Ok(())
    }

    fn item(&mut self) -> Result<(u8, u64), String> {
        let first = self.byte()?;
        let major = first >> 5;
        let additional = first & 0x1f;
        let value = match additional {
            value @ 0..=23 => u64::from(value),
            24 => {
                let value = u64::from(self.byte()?);
                if value < 24 {
                    return Err("non-canonical integer encoding".into());
                }
                value
            }
            25 => {
                let bytes = [self.byte()?, self.byte()?];
                let value = u64::from(u16::from_be_bytes(bytes));
                if value <= 0xff {
                    return Err("non-canonical integer encoding".into());
                }
                value
            }
            26 => {
                let bytes = [self.byte()?, self.byte()?, self.byte()?, self.byte()?];
                let value = u64::from(u32::from_be_bytes(bytes));
                if value <= 0xffff {
                    return Err("non-canonical integer encoding".into());
                }
                value
            }
            27 => {
                let bytes = [
                    self.byte()?,
                    self.byte()?,
                    self.byte()?,
                    self.byte()?,
                    self.byte()?,
                    self.byte()?,
                    self.byte()?,
                    self.byte()?,
                ];
                let value = u64::from_be_bytes(bytes);
                if value <= 0xffff_ffff {
                    return Err("non-canonical integer encoding".into());
                }
                value
            }
            _ => return Err("indefinite or reserved CBOR form is not allowed".into()),
        };
        Ok((major, value))
    }
}

pub fn prompt(request: &SemanticRequest, compact: bool) -> String {
    let template = if compact {
        include_str!("../prompts/semantic-ir-v1-compact.txt")
    } else {
        include_str!("../prompts/semantic-ir-v1.txt")
    };
    let actions = request
        .allowed_actions
        .iter()
        .filter_map(|id| registry::name(*id).map(|name| format!("{} {name}", id.0)))
        .collect::<Vec<_>>()
        .join("; ");
    let predicates = request
        .allowed_predicates
        .iter()
        .filter_map(|id| registry::name(*id).map(|name| format!("{} {name}", id.0)))
        .collect::<Vec<_>>()
        .join("; ");
    let entities = request
        .slots
        .iter()
        .map(|slot| {
            format!(
                "{} {} ({})",
                slot.slot,
                slot.name,
                registry::name(slot.kind).unwrap_or("UNKNOWN")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    let priors = request
        .action_priors
        .iter()
        .map(|(action, count)| format!("{}:{count}", action.0))
        .collect::<Vec<_>>()
        .join(",");
    template
        .replace("{registry_version}", &request.registry_version.to_string())
        .replace("{entities}", &entities)
        .replace(
            "{concepts}",
            &format!("Actions: {actions}\nPredicates: {predicates}"),
        )
        .replace(
            "{context}",
            &format!(
                "focus_slot={:?}; previous_action={:?}; priors={priors}; ir_version={}; registry_version={}",
                request.focus_slot,
                request.previous_action.map(|id| id.0),
                request.ir_version,
                request.registry_version
            ),
        )
        .replace("{input}", &request.input)
}
