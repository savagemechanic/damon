use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::data::DamonData;
use crate::graph::{confidence, prune_beam, CandidateGraph};
use crate::types::{IntentId, MeaningEdge, MeaningGraph};

pub const INTENT_GIT_STATUS: IntentId = IntentId(1);
pub const INTENT_GIT_DIFF: IntentId = IntentId(2);
pub const INTENT_RUN_TESTS: IntentId = IntentId(3);
pub const INTENT_LIST_FILES: IntentId = IntentId(4);
const BEAM_WIDTH: usize = 4;

#[derive(Debug)]
pub enum Interpretation {
    Resolved(MeaningGraph),
    Ambiguous(Vec<CandidateGraph>),
    Unknown { feature: u64 },
}

pub fn normalize(input: &str) -> String {
    input.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn feature_hash(input: &str) -> u64 {
    let mut h = DefaultHasher::new();
    normalize(input).hash(&mut h);
    h.finish()
}

fn lexical_evidence(text: &str, intent: IntentId) -> i32 {
    match intent {
        INTENT_RUN_TESTS => {
            if text.contains("test") { 96 }
            else if text.contains("check the code") || text.starts_with("check ") { 36 }
            else { 0 }
        }
        INTENT_GIT_DIFF => {
            if text.contains("diff") || text.contains("what changed") || text.contains("what did i change") || text.contains("what have i changed") || text.contains("touched") { 104 }
            else if text.contains("change") || text.contains("modified") { 58 }
            else { 0 }
        }
        INTENT_GIT_STATUS => {
            if text.contains("git status") || text == "status" || text.contains("repo status") { 104 }
            else if text.starts_with("check ") { 28 }
            else { 0 }
        }
        INTENT_LIST_FILES => {
            if text.contains("list files") || text.contains("show files") || text.contains("what files") { 96 }
            else { 0 }
        }
        _ => 0,
    }
}

pub fn understand(input: &str, data: &DamonData) -> Interpretation {
    let text = normalize(input);
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|w| !w.is_empty())
        .collect();
    let target = data
        .entities
        .iter()
        .find(|e| words.iter().any(|w| *w == e.name.to_ascii_lowercase()))
        .map(|e| e.id);
    let feature = feature_hash(&text);
    let learned = data.language_candidates(feature);
    let mut candidates = Vec::new();

    for intent in [INTENT_GIT_STATUS, INTENT_GIT_DIFF, INTENT_RUN_TESTS, INTENT_LIST_FILES] {
        let evidence = lexical_evidence(&text, intent);
        let prior = learned
            .iter()
            .find(|(id, _)| *id == intent)
            .map(|(_, count)| count.saturating_add(1))
            .unwrap_or(1);
        if evidence > 0 || prior > 1 {
            candidates.push(CandidateGraph::new(intent, target, evidence, prior));
        }
    }

    let beam = prune_beam(candidates, BEAM_WIDTH);
    if beam.is_empty() {
        return Interpretation::Unknown { feature };
    }

    let best = &beam[0];
    if best.evidence < 40 && best.prior <= 1 {
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
        let Interpretation::Resolved(m) = understand("show me what changed in damon", &d) else { panic!() };
        assert_eq!(m.intent, INTENT_GIT_DIFF);
        assert!(m.target.is_some());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn ambiguous_check_is_not_forced() {
        let p = std::env::temp_dir().join(format!("damon-amb-{}.data", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let d = DamonData::open(&p).unwrap();
        assert!(matches!(understand("check damon", &d), Interpretation::Ambiguous(_)));
        let _ = std::fs::remove_file(p);
    }
}
