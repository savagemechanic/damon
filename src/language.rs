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
                || text.contains("how am i connected") =>
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
        _ => 0,
    }
}

pub fn understand(input: &str, data: &DamonData) -> Interpretation {
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
    ] {
        let evidence = lexical_evidence(&text, intent);
        let prior = learned
            .iter()
            .find(|(id, _)| *id == intent)
            .map(|(_, count)| count.saturating_add(1))
            .unwrap_or(1);
        if evidence > 0 {
            let target = if matches!(intent, INTENT_NETWORK_INTERFACES | INTENT_DEFAULT_GATEWAY) {
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
