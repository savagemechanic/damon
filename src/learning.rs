use crate::data::DamonData;
use crate::types::{IntentId, MeaningGraph};

pub fn observe_verified(
    data: &mut DamonData,
    feature: u64,
    meaning: &MeaningGraph,
    success: bool,
) {
    let reward = if success { 1 } else { -1 };
    data.observe_language(feature, meaning.intent, reward, meaning.confidence);
}

pub fn observe_teacher_resolution(
    data: &mut DamonData,
    feature: u64,
    intent: IntentId,
    confidence: u8,
) {
    data.observe_language(feature, intent, 1, confidence);
}

pub fn posterior(counts: &[(IntentId, u32)]) -> Vec<(IntentId, u16)> {
    let total: u64 = counts.iter().map(|(_, c)| u64::from(*c) + 1).sum();
    if total == 0 {
        return Vec::new();
    }
    counts
        .iter()
        .map(|(intent, count)| {
            let probability = (((u64::from(*count) + 1) * 10_000) / total).min(10_000) as u16;
            (*intent, probability)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posterior_prefers_more_evidence() {
        let p = posterior(&[(IntentId(1), 9), (IntentId(2), 1)]);
        assert!(p[0].1 > p[1].1);
    }
}
