use std::env;
use std::io;
use std::path::PathBuf;
use std::time::Instant;

use crate::data::DamonData;
use crate::language::{self, candidate_to_meaning, Interpretation};
use crate::learning;
use crate::model::ModelRouter;
use crate::policy::Policy;
use crate::tools;
use crate::types::{MeaningGraph, ToolResult};

pub struct Damon {
    pub data: DamonData,
    pub models: ModelRouter,
    pub policy: Policy,
}

impl Damon {
    pub fn open_default() -> io::Result<Self> {
        let path = env::var_os("DAMON_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(default_data_path);
        Ok(Self {
            data: DamonData::open(path)?,
            models: ModelRouter::default(),
            policy: Policy::default(),
        })
    }

    pub fn handle(&mut self, input: &str) -> String {
        if let Some(result) = crate::world_commands::handle(input, &mut self.data) {
            return result;
        }
        if let Some(result) = self.maintain_memory(input) {
            return result;
        }
        let feature = language::feature_hash(input);
        let mut teacher = None;
        let meaning = match language::understand(input, &self.data) {
            Interpretation::Resolved(m) => m,
            Interpretation::Clarify(message) => return message,
            Interpretation::Ambiguous(beam) => {
                if let Some(best) = beam.first() {
                    if best.score() - beam.get(1).map(|c| c.score()).unwrap_or(best.score() - 64)
                        >= 24
                    {
                        candidate_to_meaning(best, 170)
                    } else {
                        match self.ask_teacher(input) {
                            Ok((m, provider, latency_ms)) => {
                                teacher = Some((provider, latency_ms));
                                m
                            }
                            Err(e) => return format!("I don't know how to do that yet. {e}"),
                        }
                    }
                } else {
                    return "I don't know how to do that yet.".into();
                }
            }
            Interpretation::Unknown { .. } => match self.ask_teacher(input) {
                Ok((m, provider, latency_ms)) => {
                    teacher = Some((provider, latency_ms));
                    m
                }
                Err(e) => return format!("I don't know how to do that yet. {e}"),
            },
        };

        if let Err(e) = crate::semantics::validate_request(input, &meaning, &self.data) {
            return format!("I need a clearer request: {e}");
        }
        let mut plan = match crate::reason::plan(&meaning, &self.data) {
            Ok(plan) => plan,
            Err(e) => return e,
        };
        // Check the entire plan before any tool can produce side effects.
        for action in &mut plan.actions {
            if let Err(e) = tools::prepare(action, &mut self.data) {
                return format!("Deterministic discovery failed: {e}");
            }
            if let Err(e) = self.policy.check(action) {
                return format!("Policy blocked the plan: {e}");
            }
        }
        let mut outcomes: Vec<bool> = Vec::new();
        let mut messages = Vec::new();
        let mut strategy_errors = Vec::new();
        for (index, action) in plan.actions.iter().enumerate() {
            if plan
                .dependencies
                .iter()
                .any(|d| d.step == index && d.success_required && !outcomes[d.previous])
            {
                outcomes.push(false);
                messages.push(
                    "Skipped the dependent action because its prerequisite did not pass.".into(),
                );
                continue;
            }
            let started = Instant::now();
            let result = tools::execute(action, &self.policy);
            if let Err(error) = self.data.strategies.observe(
                feature,
                crate::strategy::for_tool(action.tool),
                crate::strategy::Outcome {
                    success: result.success,
                    latency_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                    cost_units: 0,
                    used_model: false,
                    confidence: meaning.confidence,
                    risk: action_risk(action.effects),
                },
            ) {
                strategy_errors.push(error.to_string());
            }
            outcomes.push(result.success);
            messages.push(render_result(result));
        }
        let all_succeeded = outcomes.iter().all(|value| *value);
        if let Some((provider, latency_ms)) = teacher {
            let strategy = match provider {
                crate::model::ProviderKind::Ollama => crate::strategy::LOCAL_MODEL,
                crate::model::ProviderKind::External => crate::strategy::EXTERNAL_FREE_MODEL,
                crate::model::ProviderKind::Cloud => crate::strategy::CLOUD_MODEL,
            };
            if let Err(error) = self.data.strategies.observe(
                feature,
                strategy,
                crate::strategy::Outcome {
                    success: all_succeeded,
                    latency_ms,
                    cost_units: u32::from(provider == crate::model::ProviderKind::Cloud),
                    used_model: true,
                    confidence: meaning.confidence,
                    risk: plan
                        .actions
                        .iter()
                        .map(|action| action_risk(action.effects))
                        .max()
                        .unwrap_or(0),
                },
            ) {
                strategy_errors.push(error.to_string());
            }
        }
        learning::observe_verified(&mut self.data, feature, &meaning, all_succeeded);
        if !strategy_errors.is_empty() {
            messages.push(format!(
                "Strategy learning failed: {}",
                strategy_errors.join("; ")
            ));
        }
        let rendered = messages.join("\n");
        match self.data.save() {
            Ok(()) => rendered,
            Err(e) => format!(
                "{rendered}\nI could not persist learning: {e}. Reopen Damon before continuing."
            ),
        }
    }

    fn maintain_memory(&mut self, input: &str) -> Option<String> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("compact my memory") {
            return Some(match self.data.compact() {
                Ok(()) => format!("Memory compacted at generation {}.", self.data.generation()),
                Err(e) => format!("Memory compaction failed: {e}"),
            });
        }
        if input.eq_ignore_ascii_case("show memory status") {
            return Some(format!(
                "Memory generation {}: {} entities, {} learned phrases, {} retained experiences, {} cached computations, and {} learned strategy records.",
                self.data.generation(),
                self.data.entities.len(),
                self.data.language_counts.len(),
                self.data.experiences.len(),
                self.data.memo.entries.len(),
                self.data.strategies.stats.len()
            ));
        }
        for (prefix, restore) in [
            ("back up my memory to ", false),
            ("export my memory to ", false),
            ("restore my memory from ", true),
            ("import my memory from ", true),
        ] {
            if input.to_ascii_lowercase().starts_with(prefix) {
                let path = input[prefix.len()..].trim();
                let Some(path) = path
                    .strip_prefix('"')
                    .and_then(|p| p.strip_suffix('"'))
                    .filter(|p| !p.is_empty() && !p.contains('"'))
                else {
                    return Some("Put the complete backup path in double quotes.".into());
                };
                let result = if restore {
                    self.data.restore(path)
                } else {
                    self.data.export(path)
                };
                return Some(match result {
                    Ok(()) if restore => "Memory restored from a verified backup.".into(),
                    Ok(()) => "Memory backup saved.".into(),
                    Err(e) => format!("Memory operation failed: {e}"),
                });
            }
        }
        None
    }

    fn ask_teacher(
        &self,
        input: &str,
    ) -> Result<(MeaningGraph, crate::model::ProviderKind, u64), String> {
        let prompt = crate::semantics::teacher_prompt(input, &self.data);
        let started = Instant::now();
        let response = self.models.infer_validated(&prompt, |text| {
            crate::semantics::parse_teacher(text, &self.data)
                .and_then(|m| crate::semantics::validate_request(input, &m, &self.data))
        })?;
        let meaning = crate::semantics::parse_teacher(&response.text, &self.data)?;
        Ok((
            meaning,
            response.provider,
            started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        ))
    }
}

fn action_risk(effects: crate::types::Effects) -> u8 {
    if effects.contains(crate::types::Effects::DESTRUCTIVE) {
        100
    } else if effects.contains(crate::types::Effects::PRIVILEGED)
        || effects.contains(crate::types::Effects::CREDENTIAL)
    {
        80
    } else if effects.contains(crate::types::Effects::WRITE) {
        50
    } else if effects.contains(crate::types::Effects::NETWORK) {
        30
    } else if effects.contains(crate::types::Effects::PROCESS) {
        10
    } else {
        1
    }
}

fn render_result(result: ToolResult) -> String {
    let out = result.stdout.trim();
    let err = result.stderr.trim();
    if result.success {
        if out.is_empty() {
            "Done. The operation completed successfully.".into()
        } else {
            out.to_string()
        }
    } else if !err.is_empty() {
        format!(
            "The operation failed{}: {}",
            result
                .code
                .map(|c| format!(" with exit code {c}"))
                .unwrap_or_default(),
            err
        )
    } else {
        "The operation failed.".into()
    }
}

fn default_data_path() -> PathBuf {
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".damon").join("damon.data");
    }
    PathBuf::from("damon.data")
}
