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
use crate::types::MeaningGraph;

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
        if let Some(response) = conversational_response(input) {
            return response;
        }
        if let Some(result) = crate::world_commands::handle(input, &mut self.data) {
            return result;
        }
        if let Some(result) = self.maintain_memory(input) {
            return result;
        }
        if let Some(result) = self.handle_native_semantic(input) {
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
        let procedure_feature = if language::normalize(input).contains("same thing") {
            self.data.world.context.previous_feature.unwrap_or(feature)
        } else {
            feature
        };
        let learned_plan = match meaning.target {
            Some(target) => match self
                .data
                .procedures
                .plan(procedure_feature, target, &self.data)
            {
                Ok(plan) => plan,
                Err(error) => return format!("Stored procedure is invalid: {error}"),
            },
            None => None,
        };
        let mut plan = match learned_plan {
            Some(plan) => plan,
            None => match crate::reason::plan(&meaning, &self.data) {
                Ok(plan) => plan,
                Err(e) => return e,
            },
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
            let structured = crate::result_ir::ResultIr::from_tool(action, &result);
            if let Some(implementation) = self.data.capabilities.resolve_index(action.capability) {
                if let Err(error) = self
                    .data
                    .capabilities
                    .observe(implementation, result.success)
                {
                    strategy_errors.push(error.to_string());
                }
            }
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
            messages.push(structured.render_english());
        }
        let all_succeeded = outcomes.iter().all(|value| *value);
        if all_succeeded {
            if let Err(error) = self.data.procedures.remember_verified(feature, &plan) {
                strategy_errors.push(error.to_string());
            }
        } else {
            self.data.procedures.observe(procedure_feature, false);
        }
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
                "Memory generation {}: {} entities, {} capabilities, {} learned phrases, {} retained experiences, {} cached computations, and {} learned strategy records.",
                self.data.generation(),
                self.data.entities.len(),
                self.data.capabilities.capabilities.len(),
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

    fn handle_native_semantic(&mut self, input: &str) -> Option<String> {
        use crate::semantic_ir::SemanticProducer;
        let request = crate::semantic_ir::request(input, &self.data);
        let resolution = language::DeterministicProducer { data: &self.data }.resolve(&request);
        if resolution.status != crate::semantic_ir::ResolutionStatus::Resolved {
            return None;
        }
        let candidate = resolution.candidates.first()?;
        let mut plan = match crate::semantic_ir::native_plan(&candidate.ir, &request, &self.data) {
            Ok(Some(plan)) => plan,
            Ok(None) => return None,
            Err(error) => return Some(format!("I need a clearer request: {error}")),
        };
        let action = &mut plan.actions[0];
        if let Err(error) = tools::prepare(action, &mut self.data) {
            return Some(format!("Deterministic discovery failed: {error}"));
        }
        if let Err(error) = self.policy.check(action) {
            return Some(format!("Policy blocked the plan: {error}"));
        }
        let started = Instant::now();
        let result = tools::execute(action, &self.policy);
        let rendered = crate::result_ir::ResultIr::from_tool(action, &result).render_english();
        if let Some(implementation) = self.data.capabilities.resolve_index(action.capability) {
            let _ = self
                .data
                .capabilities
                .observe(implementation, result.success);
        }
        let _ = self.data.strategies.observe(
            language::feature_hash(input),
            crate::strategy::for_tool(action.tool),
            crate::strategy::Outcome {
                success: result.success,
                latency_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                cost_units: 0,
                used_model: false,
                confidence: crate::semantic_ir::computed_confidence(&resolution, 0),
                risk: action_risk(action.effects),
            },
        );
        Some(match self.data.save() {
            Ok(()) => rendered,
            Err(error) => format!(
                "{rendered}\nI could not persist learning: {error}. Reopen Damon before continuing."
            ),
        })
    }

    fn ask_teacher(
        &self,
        input: &str,
    ) -> Result<(MeaningGraph, crate::model::ProviderKind, u64), String> {
        let request = crate::semantic_ir::request(input, &self.data);
        let prompt = crate::semantic_ir::prompt(&request, false);
        let started = Instant::now();
        let response = self.models.infer_validated(&prompt, |text| {
            use crate::semantic_ir::SemanticProducer;
            let resolution =
                crate::semantic_ir::JsonSemanticProducer { output: text }.resolve(&request);
            if let Some(error) = resolution.diagnostic.as_deref() {
                return Err(error.to_string());
            }
            if resolution.status != crate::semantic_ir::ResolutionStatus::Resolved {
                return Err("teacher did not produce exactly one unambiguous candidate".into());
            }
            let meaning = crate::semantic_ir::bind(
                &resolution.candidates[0].ir,
                &request,
                &self.data,
                crate::semantic_ir::computed_confidence(&resolution, 0),
            )?;
            crate::semantics::validate_request(input, &meaning, &self.data)
        })?;
        use crate::semantic_ir::SemanticProducer;
        let resolution = crate::semantic_ir::JsonSemanticProducer {
            output: &response.text,
        }
        .resolve(&request);
        if let Some(error) = resolution.diagnostic.as_deref() {
            return Err(error.to_string());
        }
        let meaning = crate::semantic_ir::bind(
            &resolution
                .candidates
                .first()
                .ok_or("teacher returned no semantic candidate")?
                .ir,
            &request,
            &self.data,
            crate::semantic_ir::computed_confidence(&resolution, 0),
        )?;
        Ok((
            meaning,
            response.provider,
            started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        ))
    }
}

fn conversational_response(input: &str) -> Option<String> {
    let text = crate::language::normalize(input);
    if matches!(text.as_str(), "hi" | "hello" | "hey" | "hello damon") {
        return Some(
            "Hey. What would you like me to inspect, test, copy, find, or diagnose?".into(),
        );
    }
    if matches!(
        text.as_str(),
        "help" | "what can you do" | "what can you do?" | "show me what you can do"
    ) {
        return Some(
            "I can work with registered code projects, inspect Git status and diffs, list and find files, run discovered tests, copy a project file, inspect interfaces/routes/neighbors/sockets, diagnose connectivity, and manage my local memory. Try “remember project CPython at \"/path/to/cpython\"”, “run the tests in CPython”, “show me what changed”, or “What is my Mac talking to?”."
                .into(),
        );
    }
    None
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

fn default_data_path() -> PathBuf {
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".damon").join("damon.data");
    }
    PathBuf::from("damon.data")
}
