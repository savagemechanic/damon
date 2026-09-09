use crate::types::{EntityId, IntentId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Action,
    Entity,
    Concept,
    Time,
    Condition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub value: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from: u16,
    pub relation: u16,
    pub to: u16,
}

#[derive(Debug, Clone)]
pub struct CandidateGraph {
    pub intent: IntentId,
    pub target: Option<EntityId>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub evidence: i32,
    pub prior: u32,
}

impl CandidateGraph {
    pub fn new(intent: IntentId, target: Option<EntityId>, evidence: i32, prior: u32) -> Self {
        let mut nodes = vec![Node {
            kind: NodeKind::Action,
            value: intent.0,
        }];
        let mut edges = Vec::new();
        if let Some(entity) = target {
            nodes.push(Node {
                kind: NodeKind::Entity,
                value: entity.0,
            });
            edges.push(Edge {
                from: 0,
                relation: 1,
                to: 1,
            });
        }
        let concept = match intent.0 {
            1 => 4,
            2 => 3,
            3 => 1,
            4 | 5 => 2,
            6 => 5,
            7 => 6,
            _ => 0,
        };
        if concept != 0 {
            let to = nodes.len() as u16;
            nodes.push(Node {
                kind: NodeKind::Concept,
                value: concept,
            });
            edges.push(Edge {
                from: 0,
                relation: crate::semantics::Relation::Object as u16,
                to,
            });
        }
        if intent.0 == 5 {
            let to = nodes.len() as u16;
            nodes.push(Node {
                kind: NodeKind::Time,
                value: 1,
            });
            edges.push(Edge {
                from: 0,
                relation: crate::semantics::Relation::Time as u16,
                to,
            });
        }
        Self {
            intent,
            target,
            nodes,
            edges,
            evidence,
            prior,
        }
    }

    pub fn score(&self) -> i64 {
        self.evidence as i64 + integer_log2(self.prior.max(1)) as i64 * 8
    }
}

pub fn prune_beam(mut candidates: Vec<CandidateGraph>, width: usize) -> Vec<CandidateGraph> {
    candidates.sort_by_key(|c| std::cmp::Reverse(c.score()));
    candidates.truncate(width.max(1));
    candidates
}

pub fn confidence(beam: &[CandidateGraph]) -> u8 {
    let Some(best) = beam.first() else {
        return 0;
    };
    let second = beam
        .get(1)
        .map(CandidateGraph::score)
        .unwrap_or(best.score() - 32);
    let margin = (best.score() - second).max(0) as u64;
    (128 + margin.min(127)) as u8
}

fn integer_log2(v: u32) -> u32 {
    31 - v.leading_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn beam_keeps_best_graph() {
        let a = CandidateGraph::new(IntentId(1), None, 10, 1);
        let b = CandidateGraph::new(IntentId(2), None, 80, 1);
        let beam = prune_beam(vec![a, b], 1);
        assert_eq!(beam[0].intent, IntentId(2));
    }
}
