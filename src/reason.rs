use crate::data::DamonData;
use crate::tools;
use crate::types::{Action, MeaningGraph};

pub fn resolve(meaning: &MeaningGraph, data: &DamonData) -> Result<Action, String> {
    tools::action_from_meaning(meaning, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::INTENT_GIT_DIFF;
    use crate::types::MeaningGraph;

    #[test]
    fn known_meaning_resolves_to_action() {
        let p = std::env::temp_dir().join(format!("damon-reason-{}.data", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let data = DamonData::open(&p).unwrap();
        let meaning = MeaningGraph {
            intent: INTENT_GIT_DIFF,
            target: data.resolve("damon"),
            edges: Vec::new(),
            confidence: 200,
        };
        assert!(resolve(&meaning, &data).is_ok());
        let _ = std::fs::remove_file(p);
    }
}
