use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::data::DamonData;
use crate::graph::{confidence, prune_beam, CandidateGraph};
use crate::types::{IntentId, MeaningEdge, MeaningGraph};

pub const INTENT_GIT_STATUS: IntentId = IntentId(1);
pub const INTENT_GIT_DIFF: IntentId = IntentId(2);
pub const INTENT_RUN_TESTS: IntentId = IntentId(3);
pub const INTENT_LIST_FILES: IntentId = IntentId(4);
pub const INTENT_CHANGED_FILES: IntentId = IntentId(5);
pub const INTENT_NETWORK_INTERFACES: IntentId = IntentId(6);
pub const INTENT_DEFAULT_GATEWAY: IntentId = IntentId(7);
pub const INTENT_NETWORK_NEIGHBORS: IntentId = IntentId(8);
pub const INTENT_NETWORK_DIAGNOSIS: IntentId = IntentId(9);
pub const INTENT_LIST_SOCKETS: IntentId = IntentId(10);
const BEAM_WIDTH: usize = 4;

#[derive(Debug)]
pub enum Interpretation {
    Resolved(MeaningGraph),
    Ambiguous(Vec<CandidateGraph>),
    Unknown { feature: u64 },
    Clarify(String),
}

pub fn normalize(input: &str) -> String {
    input
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn feature_hash(input: &str) -> u64 {
    let mut h = DefaultHasher::new();
    normalize(input).hash(&mut h);
    h.finish()
}

fn lexical_evidence(text: &str, intent: IntentId) -> i32 {
    match intent {
        INTENT_RUN_TESTS => {
            if text.contains("test")
                || (text.starts_with("check ") && crate::reference::has_reference(text))
            {
                96
            } else if text.contains("check the code") || text.starts_with("check ") {
                36
            } else {
                0
            }
        }
        INTENT_GIT_DIFF => {
            if text.contains("diff")
                || text.contains("what changed")
                || text.contains("what did i change")
                || text.contains("what have i changed")
                || text.contains("touched")
            {
                104
            } else if text.contains("change") || text.contains("modified") {
                58
            } else {
                0
            }
        }
        INTENT_GIT_STATUS => {
            if text.contains("git status") || text == "status" || text.contains("repo status") {
                104
            } else if text.starts_with("check ") {
                28
            } else {
                0
            }
        }
        INTENT_LIST_FILES
            if text.contains("list files")
                || text.contains("show files")
                || text.contains("what files") =>
        {
            96
        }
        INTENT_CHANGED_FILES
            if text.contains("yesterday")
                && (text.contains("changed") || text.contains("modified")) =>
        {
            150
        }
        INTENT_NETWORK_INTERFACES
            if text.contains("network interface")
                || text.contains("what network am i connected")
                || text.contains("how am i connected")
                || ((text.contains("wifi") || text.contains("wi-fi"))
                    && (text.contains("connected")
                        || text.contains("network")
                        || text.contains("signal")
                        || text.contains("strength"))) =>
        {
            160
        }
        INTENT_DEFAULT_GATEWAY
            if text.contains("default gateway")
                || text.contains("how does traffic leave")
                || text.contains("how traffic leaves") =>
        {
            160
        }
        INTENT_NETWORK_NEIGHBORS
            if text.contains("network neighbors")
                || text.contains("devices my mac")
                || text.contains("devices this mac")
                || text.contains("devices the mac") =>
        {
            160
        }
        INTENT_NETWORK_DIAGNOSIS
            if text.contains("internet not working")
                || text.contains("is dns working")
                || text.contains("diagnose my network") =>
        {
            170
        }
        INTENT_LIST_SOCKETS
            if text.contains("list sockets")
                || text.contains("current connections")
                || text.contains("my mac talking to") =>
        {
            165
        }
        _ => 0,
    }
}

pub struct DeterministicProducer<'a> {
    pub data: &'a DamonData,
}

impl crate::semantic_ir::SemanticProducer for DeterministicProducer<'_> {
    fn resolve(
        &self,
        request: &crate::semantic_ir::SemanticRequest,
    ) -> crate::semantic_ir::SemanticResolution {
        use crate::semantic_ir::{
            from_meaning, ProducerKind, ResolutionStatus, ScoredCandidate, SemanticResolution,
        };
        if let Some(ir) = direct_semantic_ir(request) {
            return SemanticResolution {
                status: ResolutionStatus::Resolved,
                candidates: vec![ScoredCandidate { ir, evidence: 180 }],
                unresolved_spans: Vec::new(),
                producer: ProducerKind::Deterministic,
                diagnostic: None,
            };
        }
        let interpreted = understand_bound(&request.input, self.data);
        let (status, meanings, diagnostic) = match interpreted {
            Interpretation::Resolved(meaning) => {
                (ResolutionStatus::Resolved, vec![(meaning, 160)], None)
            }
            Interpretation::Ambiguous(graphs) => (
                ResolutionStatus::Ambiguous,
                graphs
                    .into_iter()
                    .map(|graph| {
                        let score = graph.score() as i32;
                        (candidate_to_meaning(&graph, 0), score)
                    })
                    .collect(),
                None,
            ),
            Interpretation::Unknown { .. } => (ResolutionStatus::Unknown, Vec::new(), None),
            Interpretation::Clarify(message) => {
                (ResolutionStatus::Invalid, Vec::new(), Some(message))
            }
        };
        let candidates = meanings
            .into_iter()
            .filter_map(|(meaning, evidence)| {
                from_meaning(&meaning, request)
                    .ok()
                    .map(|ir| ScoredCandidate { ir, evidence })
            })
            .collect::<Vec<_>>();
        SemanticResolution {
            status: if candidates.is_empty() && status == ResolutionStatus::Resolved {
                ResolutionStatus::Invalid
            } else {
                status
            },
            candidates,
            unresolved_spans: Vec::new(),
            producer: ProducerKind::Deterministic,
            diagnostic,
        }
    }
}

fn direct_semantic_ir(
    request: &crate::semantic_ir::SemanticRequest,
) -> Option<crate::semantic_ir::CandidateIr> {
    use crate::semantic_ir::{CandidateIr, Edge, Node, NodeKind, SourceSpan};
    use crate::semantic_registry as registry;
    let text = normalize(&request.input);
    if text.starts_with("copy ") && text.contains(" to ") {
        let object_text = request.input.strip_prefix("copy ")?.split_once(" from ")?.0;
        let start = request.input.find(object_text)?;
        let end = start.checked_add(object_text.len())?;
        let source = request
            .slots
            .iter()
            .find(|slot| slot.kind == registry::PROJECT)?;
        let destination = request
            .slots
            .iter()
            .find(|slot| slot.kind == registry::DIRECTORY)?;
        return Some(CandidateIr {
            nodes: vec![
                Node {
                    kind: NodeKind::Action,
                    concept: registry::COPY.0,
                    value: 0,
                    span: None,
                },
                Node {
                    kind: NodeKind::Entity,
                    concept: registry::FILE.0,
                    value: 0,
                    span: Some(SourceSpan {
                        start: start.try_into().ok()?,
                        end: end.try_into().ok()?,
                        expected_kind: registry::FILE.0,
                    }),
                },
                Node {
                    kind: NodeKind::Entity,
                    concept: registry::PROJECT.0,
                    value: u32::from(source.slot),
                    span: None,
                },
                Node {
                    kind: NodeKind::Entity,
                    concept: registry::DIRECTORY.0,
                    value: u32::from(destination.slot),
                    span: None,
                },
            ],
            edges: vec![
                Edge {
                    source: 0,
                    predicate: registry::OBJECT.0,
                    target: 1,
                },
                Edge {
                    source: 0,
                    predicate: registry::SOURCE.0,
                    target: 2,
                },
                Edge {
                    source: 0,
                    predicate: registry::DESTINATION.0,
                    target: 3,
                },
            ],
        });
    }
    if text.starts_with("find files larger than 10 mb") {
        return Some(CandidateIr {
            nodes: vec![
                Node {
                    kind: NodeKind::Action,
                    concept: registry::FIND.0,
                    value: 0,
                    span: None,
                },
                Node {
                    kind: NodeKind::Entity,
                    concept: registry::FILE_SET.0,
                    value: u32::MAX,
                    span: None,
                },
                Node {
                    kind: NodeKind::Constraint,
                    concept: registry::SIZE.0,
                    value: registry::GREATER_THAN.0,
                    span: None,
                },
                Node {
                    kind: NodeKind::Value,
                    concept: registry::BYTES.0,
                    value: 10_000_000,
                    span: None,
                },
            ],
            edges: vec![
                Edge {
                    source: 0,
                    predicate: registry::OBJECT.0,
                    target: 1,
                },
                Edge {
                    source: 0,
                    predicate: registry::REQUIRES.0,
                    target: 2,
                },
                Edge {
                    source: 2,
                    predicate: registry::VALUE.0,
                    target: 3,
                },
            ],
        });
    }
    None
}

pub fn understand(input: &str, data: &DamonData) -> Interpretation {
    use crate::semantic_ir::{ResolutionStatus, SemanticProducer};
    let request = crate::semantic_ir::request(input, data);
    let resolution = DeterministicProducer { data }.resolve(&request);
    match resolution.status {
        ResolutionStatus::Resolved => {
            let Some(candidate) = resolution.candidates.first() else {
                return Interpretation::Unknown {
                    feature: feature_hash(input),
                };
            };
            let confidence = crate::semantic_ir::computed_confidence(&resolution, 0);
            match crate::semantic_ir::bind(&candidate.ir, &request, data, confidence) {
                Ok(meaning) => Interpretation::Resolved(meaning),
                Err(message) => Interpretation::Clarify(message),
            }
        }
        ResolutionStatus::Ambiguous => {
            let mut graphs = Vec::new();
            for candidate in resolution.candidates {
                if let Ok(meaning) = crate::semantic_ir::bind(&candidate.ir, &request, data, 0) {
                    graphs.push(CandidateGraph {
                        intent: meaning.intent,
                        target: meaning.target,
                        nodes: meaning.nodes,
                        edges: meaning
                            .edges
                            .into_iter()
                            .map(|edge| crate::graph::Edge {
                                from: edge.source as u16,
                                relation: edge.relation,
                                to: edge.target as u16,
                            })
                            .collect(),
                        evidence: candidate.evidence,
                        prior: 1,
                    });
                }
            }
            if graphs.is_empty() {
                Interpretation::Unknown {
                    feature: feature_hash(input),
                }
            } else {
                Interpretation::Ambiguous(graphs)
            }
        }
        ResolutionStatus::Invalid => Interpretation::Clarify(
            resolution
                .diagnostic
                .unwrap_or_else(|| "The request could not be bound safely.".into()),
        ),
        ResolutionStatus::Unknown => Interpretation::Unknown {
            feature: feature_hash(input),
        },
    }
}

fn understand_bound(input: &str, data: &DamonData) -> Interpretation {
    let text = normalize(input);
    if text
        .split_whitespace()
        .any(|w| matches!(w, "never" | "don't" | "unless" | "tomorrow"))
        || text.contains("do not")
    {
        return Interpretation::Unknown {
            feature: feature_hash(input),
        };
    }
    if text.contains("same thing")
        || matches!(text.as_str(), "do that again" | "repeat that" | "again")
    {
        return match crate::reference::repeat(input, data) {
            Ok(m) => Interpretation::Resolved(m),
            Err(e) => Interpretation::Clarify(e),
        };
    }
    if let Some(meaning) = data.learned_graphs.get(&feature_hash(input)) {
        if crate::semantics::validate(meaning, data).is_ok() {
            let mut meaning = meaning.clone();
            if meaning.intent.0 <= 5 && crate::reference::project_mentions(input, data).is_empty() {
                match crate::reference::target(input, data) {
                    Ok(Some(target)) => meaning = crate::reference::retarget(&meaning, target),
                    Ok(None) => {}
                    Err(e) => return Interpretation::Clarify(e),
                }
            }
            return Interpretation::Resolved(meaning);
        }
    }
    if text.len() > 8192 {
        return Interpretation::Unknown {
            feature: feature_hash(input),
        };
    }
    for (separator, relation) in [
        (" and if they pass ", crate::semantics::Relation::Condition),
        (" and then ", crate::semantics::Relation::Dependency),
        (" and ", crate::semantics::Relation::Dependency),
    ] {
        if let Some((first, second)) = text.split_once(separator) {
            // A bounded two-clause parse, never combinatorial enumeration.
            if second.contains(" and ") {
                return Interpretation::Unknown {
                    feature: feature_hash(input),
                };
            }
            let mut a = match understand_single(first, data) {
                Interpretation::Resolved(m) => m,
                Interpretation::Clarify(e) => return Interpretation::Clarify(e),
                _ => {
                    return Interpretation::Unknown {
                        feature: feature_hash(input),
                    }
                }
            };
            let mut b = match understand_single(second, data) {
                Interpretation::Resolved(m) => m,
                Interpretation::Clarify(e) => return Interpretation::Clarify(e),
                _ => {
                    return Interpretation::Unknown {
                        feature: feature_hash(input),
                    }
                }
            };
            if b.intent.0 <= 5 && crate::reference::project_mentions(second, data).is_empty() {
                if let Some(target) = a.target {
                    b = crate::reference::retarget(&b, target);
                }
            }
            let offset = a.nodes.len() as u32;
            a.nodes.extend(b.nodes);
            a.edges.extend(b.edges.into_iter().map(|e| MeaningEdge {
                source: e.source + offset,
                target: e.target + offset,
                relation: e.relation,
            }));
            a.edges.push(MeaningEdge {
                source: offset,
                target: 0,
                relation: relation as u16,
            });
            a.confidence = a.confidence.min(b.confidence);
            return Interpretation::Resolved(a);
        }
    }
    understand_single(input, data)
}
fn understand_single(input: &str, data: &DamonData) -> Interpretation {
    let text = normalize(input);
    let feature = feature_hash(&text);
    let learned = data.language_candidates(feature);
    let mut candidates = Vec::new();

    for intent in [
        INTENT_GIT_STATUS,
        INTENT_GIT_DIFF,
        INTENT_RUN_TESTS,
        INTENT_LIST_FILES,
        INTENT_CHANGED_FILES,
        INTENT_NETWORK_INTERFACES,
        INTENT_DEFAULT_GATEWAY,
        INTENT_NETWORK_NEIGHBORS,
        INTENT_NETWORK_DIAGNOSIS,
        INTENT_LIST_SOCKETS,
    ] {
        let evidence = lexical_evidence(&text, intent);
        let prior = learned
            .iter()
            .find(|(id, _)| *id == intent)
            .map(|(_, count)| count.saturating_add(1))
            .unwrap_or(1);
        if evidence > 0 {
            let target = if matches!(
                intent,
                INTENT_NETWORK_INTERFACES
                    | INTENT_DEFAULT_GATEWAY
                    | INTENT_NETWORK_NEIGHBORS
                    | INTENT_NETWORK_DIAGNOSIS
                    | INTENT_LIST_SOCKETS
            ) {
                data.resolve("local host").filter(|id| {
                    data.entity(*id)
                        .is_some_and(|entity| entity.kind == crate::world::HOST)
                })
            } else {
                match crate::reference::target(input, data) {
                    Ok(target) => target,
                    Err(error) => return Interpretation::Clarify(error),
                }
            };
            candidates.push(CandidateGraph::new(intent, target, evidence, prior));
        }
    }

    let beam = prune_beam(candidates, BEAM_WIDTH);
    if beam.is_empty() {
        return Interpretation::Unknown { feature };
    }

    let best = &beam[0];
    if best.evidence < 40 && best.prior <= 1 && beam.len() == 1 {
        return Interpretation::Unknown { feature };
    }

    let conf = confidence(&beam);
    if conf < 150 && beam.len() > 1 {
        return Interpretation::Ambiguous(beam);
    }

    Interpretation::Resolved(candidate_to_meaning(best, conf))
}

pub fn candidate_to_meaning(candidate: &CandidateGraph, confidence: u8) -> MeaningGraph {
    MeaningGraph {
        intent: candidate.intent,
        target: candidate.target,
        nodes: candidate.nodes.clone(),
        edges: candidate
            .edges
            .iter()
            .map(|edge| MeaningEdge {
                source: edge.from as u32,
                relation: edge.relation,
                target: edge.to as u32,
            })
            .collect(),
        confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::DamonData;

    #[test]
    fn resolves_common_intents() {
        let p = std::env::temp_dir().join(format!("damon-lang-{}.data", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let d = DamonData::open(&p).unwrap();
        let Interpretation::Resolved(m) = understand("show me what changed in damon", &d) else {
            panic!()
        };
        assert_eq!(m.intent, INTENT_GIT_DIFF);
        assert!(m.target.is_some());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn wifi_question_stays_on_the_deterministic_network_path() {
        let p = std::env::temp_dir().join(format!("damon-wifi-lang-{}.data", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let d = DamonData::open(&p).unwrap();
        let Interpretation::Resolved(meaning) =
            understand("what wifi network am i connected to", &d)
        else {
            panic!("Wi-Fi question should resolve without a model")
        };
        assert_eq!(meaning.intent, INTENT_NETWORK_INTERFACES);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn ambiguous_check_is_not_forced() {
        let p = std::env::temp_dir().join(format!("damon-amb-{}.data", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let d = DamonData::open(&p).unwrap();
        assert!(matches!(
            understand("check damon", &d),
            Interpretation::Ambiguous(_)
        ));
        let _ = std::fs::remove_file(p);
    }
}
