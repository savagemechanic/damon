//! Compact count-based strategy learning. This ranks choices; it never grants
//! permission, supplies tool arguments, or substitutes for verification.
use crate::{
    codec::{write_u32, write_u64, Reader},
    storage,
};
use std::{collections::HashMap, io, io::Write};

pub const INSPECT: u8 = 1;
pub const SEARCH: u8 = 2;
pub const TEST: u8 = 3;
pub const DETERMINISTIC_TOOL: u8 = 4;
pub const LOCAL_MODEL: u8 = 5;
pub const EXTERNAL_FREE_MODEL: u8 = 6;
pub const ASK_USER: u8 = 7;
pub const CLOUD_MODEL: u8 = 8;
const MAX_STATS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub success: bool,
    pub latency_ms: u64,
    pub cost_units: u32,
    pub used_model: bool,
    pub confidence: u8,
    pub risk: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stat {
    pub state_hash: u64,
    pub strategy: u8,
    pub successes: u32,
    pub failures: u32,
    pub latency_ms: u64,
    pub cost_units: u64,
    pub model_calls: u32,
    pub confidence_total: u64,
    pub risk_total: u64,
}

#[derive(Clone, Debug, Default)]
pub struct StrategyTable {
    pub stats: Vec<Stat>,
    index: HashMap<(u64, u8), usize>,
}

impl StrategyTable {
    pub fn observe(&mut self, state_hash: u64, strategy: u8, outcome: Outcome) -> io::Result<()> {
        if !valid_strategy(strategy) {
            return Err(storage::invalid("unknown strategy"));
        }
        let index = if let Some(index) = self.index.get(&(state_hash, strategy)).copied() {
            index
        } else {
            if self.stats.len() >= MAX_STATS {
                let remove = self
                    .stats
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, stat)| {
                        (
                            stat.successes.saturating_add(stat.failures),
                            stat.state_hash,
                            stat.strategy,
                        )
                    })
                    .map(|(index, _)| index)
                    .ok_or_else(|| storage::invalid("strategy table is full"))?;
                self.stats.swap_remove(remove);
                self.rebuild_index();
            }
            let index = self.stats.len();
            self.stats.push(Stat {
                state_hash,
                strategy,
                successes: 0,
                failures: 0,
                latency_ms: 0,
                cost_units: 0,
                model_calls: 0,
                confidence_total: 0,
                risk_total: 0,
            });
            self.index.insert((state_hash, strategy), index);
            index
        };
        let stat = &mut self.stats[index];
        if outcome.success {
            stat.successes = stat.successes.saturating_add(1);
        } else {
            stat.failures = stat.failures.saturating_add(1);
        }
        stat.latency_ms = stat.latency_ms.saturating_add(outcome.latency_ms);
        stat.cost_units = stat
            .cost_units
            .saturating_add(u64::from(outcome.cost_units));
        if outcome.used_model {
            stat.model_calls = stat.model_calls.saturating_add(1);
        }
        stat.confidence_total = stat
            .confidence_total
            .saturating_add(u64::from(outcome.confidence));
        stat.risk_total = stat.risk_total.saturating_add(u64::from(outcome.risk));
        Ok(())
    }

    pub fn rank(&self, state_hash: u64, choices: &[u8]) -> Vec<(u8, i64)> {
        let mut ranked = choices
            .iter()
            .copied()
            .filter(|strategy| valid_strategy(*strategy))
            .map(|strategy| (strategy, self.score(state_hash, strategy)))
            .collect::<Vec<_>>();
        ranked.sort_by_key(|(strategy, score)| (std::cmp::Reverse(*score), *strategy));
        ranked
    }

    pub fn stat(&self, state_hash: u64, strategy: u8) -> Option<&Stat> {
        self.index
            .get(&(state_hash, strategy))
            .and_then(|index| self.stats.get(*index))
    }

    fn score(&self, state_hash: u64, strategy: u8) -> i64 {
        let base = match strategy {
            INSPECT => 900,
            SEARCH => 850,
            TEST => 800,
            DETERMINISTIC_TOOL => 750,
            LOCAL_MODEL => 500,
            EXTERNAL_FREE_MODEL => 400,
            ASK_USER => 300,
            CLOUD_MODEL => 100,
            _ => 0,
        };
        let Some(stat) = self.stat(state_hash, strategy) else {
            return base;
        };
        let observations = u64::from(stat.successes) + u64::from(stat.failures);
        let posterior = (u64::from(stat.successes) + 1) * 200 / (observations + 2);
        let latency = stat.latency_ms / observations.max(1) / 50;
        let confidence = stat.confidence_total / observations.max(1) / 8;
        let risk = stat.risk_total / observations.max(1);
        base + posterior as i64 + confidence as i64
            - latency.min(100) as i64
            - (stat.cost_units / observations.max(1)).min(100) as i64
            - risk.min(100) as i64
    }

    fn rebuild_index(&mut self) {
        self.index.clear();
        for (index, stat) in self.stats.iter().enumerate() {
            self.index.insert((stat.state_hash, stat.strategy), index);
        }
    }

    pub(crate) fn encode(&self, writer: &mut impl Write) -> io::Result<()> {
        write_u32(writer, self.stats.len() as u32)?;
        let mut rows = self.stats.iter().collect::<Vec<_>>();
        rows.sort_by_key(|stat| (stat.state_hash, stat.strategy));
        for stat in rows {
            write_u64(writer, stat.state_hash)?;
            writer.write_all(&[stat.strategy])?;
            write_u32(writer, stat.successes)?;
            write_u32(writer, stat.failures)?;
            write_u64(writer, stat.latency_ms)?;
            write_u64(writer, stat.cost_units)?;
            write_u32(writer, stat.model_calls)?;
            write_u64(writer, stat.confidence_total)?;
            write_u64(writer, stat.risk_total)?;
        }
        Ok(())
    }

    pub(crate) fn decode(reader: &mut Reader<'_>) -> io::Result<Self> {
        let count = reader.u32()? as usize;
        if count > MAX_STATS {
            return Err(storage::invalid("too many strategy statistics"));
        }
        let mut table = Self::default();
        for _ in 0..count {
            let stat = Stat {
                state_hash: reader.u64()?,
                strategy: reader.u8()?,
                successes: reader.u32()?,
                failures: reader.u32()?,
                latency_ms: reader.u64()?,
                cost_units: reader.u64()?,
                model_calls: reader.u32()?,
                confidence_total: reader.u64()?,
                risk_total: reader.u64()?,
            };
            if !valid_strategy(stat.strategy)
                || table
                    .index
                    .insert((stat.state_hash, stat.strategy), table.stats.len())
                    .is_some()
            {
                return Err(storage::invalid("invalid or duplicate strategy statistic"));
            }
            table.stats.push(stat);
        }
        Ok(table)
    }
}

fn valid_strategy(strategy: u8) -> bool {
    (INSPECT..=CLOUD_MODEL).contains(&strategy)
}

pub fn for_tool(tool: crate::types::ToolId) -> u8 {
    match tool.0 {
        1 | 2 | 4 => INSPECT,
        3 => TEST,
        5 => SEARCH,
        _ => DETERMINISTIC_TOOL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_outcomes_change_ranking_without_crossing_cost_tiers() {
        let mut table = StrategyTable::default();
        let state = 42;
        for _ in 0..8 {
            table
                .observe(
                    state,
                    TEST,
                    Outcome {
                        success: false,
                        latency_ms: 5000,
                        cost_units: 0,
                        used_model: false,
                        confidence: 80,
                        risk: 20,
                    },
                )
                .unwrap();
            table
                .observe(
                    state,
                    SEARCH,
                    Outcome {
                        success: true,
                        latency_ms: 10,
                        cost_units: 0,
                        used_model: false,
                        confidence: 240,
                        risk: 1,
                    },
                )
                .unwrap();
        }
        assert_eq!(table.rank(state, &[TEST, SEARCH])[0].0, SEARCH);
        assert_eq!(
            table.rank(state, &[LOCAL_MODEL, CLOUD_MODEL])[0].0,
            LOCAL_MODEL
        );
    }
}
