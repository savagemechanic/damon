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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Processing,
    AskingOllama,
    AskingCloud,
    Thinking,
    CheckingMeaning,
    Running,
    Verifying,
    Learning,
    Ready,
}

impl RuntimeState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Processing => "processing",
            Self::AskingOllama => "asking_ollama",
            Self::AskingCloud => "asking_cloud",
            Self::Thinking => "thinking",
            Self::CheckingMeaning => "checking_meaning",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Learning => "learning",
            Self::Ready => "ready",
        }
    }
}

impl Damon {
    pub fn open_default() -> io::Result<Self> {
        let path = env::var_os("DAMON_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(default_data_path);
        Self::open(path)
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
        let data = DamonData::open(path)?;
        let mut policy = Policy::default();
        policy.approvals = data.approvals.clone();
        Ok(Self {
            data,
            models: ModelRouter::default(),
            policy,
        })
    }

    pub fn handle(&mut self, input: &str) -> String {
        self.handle_with_events(input, |_| {})
    }

    pub fn handle_with_events(
        &mut self,
        input: &str,
        mut event: impl FnMut(RuntimeState),
    ) -> String {
        event(RuntimeState::Processing);
        let response = self.handle_inner(input, &mut event);
        event(RuntimeState::Ready);
        response
    }

    fn handle_inner(&mut self, input: &str, event: &mut impl FnMut(RuntimeState)) -> String {
        if matches!(
            crate::language::normalize(input).as_str(),
            "forget my approvals" | "revoke my approvals" | "clear my approvals"
        ) {
            let previous = self.policy.approvals.clone();
            self.policy.approvals.revoke_all();
            self.policy.clear_pending();
            self.data.approvals = self.policy.approvals.clone();
            return match self.data.save() {
                Ok(()) => "I revoked every remembered policy approval.".into(),
                Err(error) => {
                    self.policy.approvals = previous.clone();
                    self.data.approvals = previous;
                    format!("I could not save the revocation, so no approvals changed: {error}")
                }
            };
        }
        if is_policy_confirmation(input) {
            let Some(pending) = self.policy.take_pending() else {
                return "There is no pending action to approve.".into();
            };
            let previous = self.policy.approvals.clone();
            if let Err(error) = self.policy.approve(&pending.action) {
                return format!("I could not record that approval: {error}");
            }
            self.data.approvals = self.policy.approvals.clone();
            if let Err(error) = self.data.save() {
                self.policy.approvals = previous.clone();
                self.data.approvals = previous;
                return format!(
                    "I understood your approval but could not save it, so I did not execute the action: {error}"
                );
            }
            return self.handle_inner(&pending.input, event);
        }
        self.policy.clear_pending();
        if let Some(response) = conversational_response(input) {
            return response;
        }
        if let Some(result) = crate::world_commands::handle(input, &mut self.data) {
            return result;
        }
        if let Some(result) = self.maintain_memory(input) {
            return result;
        }
        if let Some(result) = self.handle_native_semantic(input, event) {
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
                        match self.ask_teacher(input, event) {
                            Ok((m, provider, latency_ms)) => {
                                teacher = Some((provider, latency_ms));
                                m
                            }
                            Err(error) => return teacher_unavailable(&error),
                        }
                    }
                } else {
                    return "I don't know how to do that yet.".into();
                }
            }
            Interpretation::Unknown { .. } => match self.ask_teacher(input, event) {
                Ok((m, provider, latency_ms)) => {
                    teacher = Some((provider, latency_ms));
                    m
                }
                Err(error) => return teacher_unavailable(&error),
            },
        };

        event(RuntimeState::CheckingMeaning);
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
                self.policy.remember_pending(action, input);
                return format!("Policy blocked the plan: {e}");
            }
        }
        let mut outcomes: Vec<bool> = Vec::new();
        let mut messages = Vec::new();
        let mut strategy_errors = Vec::new();
        event(RuntimeState::Running);
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
            event(RuntimeState::Verifying);
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
        event(RuntimeState::Learning);
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

    fn handle_native_semantic(
        &mut self,
        input: &str,
        event: &mut impl FnMut(RuntimeState),
    ) -> Option<String> {
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
        event(RuntimeState::CheckingMeaning);
        if let Err(error) = tools::prepare(action, &mut self.data) {
            return Some(format!("Deterministic discovery failed: {error}"));
        }
        if let Err(error) = self.policy.check(action) {
            self.policy.remember_pending(action, input);
            return Some(format!("Policy blocked the plan: {error}"));
        }
        let started = Instant::now();
        event(RuntimeState::Running);
        let result = tools::execute(action, &self.policy);
        event(RuntimeState::Verifying);
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
        event(RuntimeState::Learning);
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
        event: &mut impl FnMut(RuntimeState),
    ) -> Result<(MeaningGraph, crate::model::ProviderKind, u64), String> {
        let request = crate::semantic_ir::request(input, &self.data);
        let prompt = crate::semantic_ir::prompt(&request, false);
        let schema = crate::semantic_ir::json_schema(&request);
        let started = Instant::now();
        let first = self.models.infer_structured_validated_with_events(
            &prompt,
            &schema,
            |text| validate_teacher_output(input, text, &request, &self.data),
            |model_event| match model_event {
                crate::model::ModelEvent::AskingOllama => event(RuntimeState::AskingOllama),
                crate::model::ModelEvent::AskingCloud => event(RuntimeState::AskingCloud),
                crate::model::ModelEvent::Thinking => event(RuntimeState::Thinking),
                crate::model::ModelEvent::Processing => event(RuntimeState::Processing),
            },
        );
        let response = match first {
            Ok(response) => response,
            Err(first_error) => {
                let repair = format!(
                    "{prompt}\nYour previous candidate was rejected: {first_error}. Make one final fresh attempt. Follow the schema exactly; return meaning only."
                );
                self.models
                    .infer_structured_validated_with_events(
                        &repair,
                        &schema,
                        |text| validate_teacher_output(input, text, &request, &self.data),
                        |model_event| match model_event {
                            crate::model::ModelEvent::AskingOllama => {
                                event(RuntimeState::AskingOllama)
                            }
                            crate::model::ModelEvent::AskingCloud => {
                                event(RuntimeState::AskingCloud)
                            }
                            crate::model::ModelEvent::Thinking => event(RuntimeState::Thinking),
                            crate::model::ModelEvent::Processing => event(RuntimeState::Processing),
                        },
                    )
                    .map_err(|second_error| {
                        format!("{first_error}; repair failed: {second_error}")
                    })?
            }
        };
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

fn validate_teacher_output(
    input: &str,
    text: &str,
    request: &crate::semantic_ir::SemanticRequest,
    data: &DamonData,
) -> Result<(), String> {
    use crate::semantic_ir::SemanticProducer;
    let resolution = crate::semantic_ir::JsonSemanticProducer { output: text }.resolve(request);
    if let Some(error) = resolution.diagnostic.as_deref() {
        return Err(error.to_string());
    }
    if resolution.status != crate::semantic_ir::ResolutionStatus::Resolved {
        return Err("teacher did not produce exactly one unambiguous candidate".into());
    }
    let meaning = crate::semantic_ir::bind(
        &resolution.candidates[0].ir,
        request,
        data,
        crate::semantic_ir::computed_confidence(&resolution, 0),
    )?;
    crate::semantics::validate_request(input, &meaning, data)
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

fn is_policy_confirmation(input: &str) -> bool {
    matches!(
        crate::language::normalize(input).as_str(),
        "do it" | "yes do it" | "allow it" | "approve it"
    )
}

fn teacher_unavailable(error: &str) -> String {
    let detail = if error.contains("No language teacher is configured") {
        "No language model is configured. Add an OpenCode Zen API key."
    } else if error.contains("Cloud:") || error.contains("OpenCode Zen") {
        if error.contains("MissingSessionID")
            || error.contains("free tier can only be used in OpenCode")
        {
            "OpenCode Zen restricts the selected free model to its own client, so Damon cannot use it through the API."
        } else if error.contains("Model is unavailable") {
            "The selected OpenCode Zen model is currently unavailable. Refresh the model list or select another API-accessible model."
        } else if error.contains("401") || error.contains("403") {
            "OpenCode Zen rejected the API key or model access. Replace the key or select an enabled model."
        } else if error.contains("timed out") || error.contains("connect") {
            "I couldn't reach OpenCode Zen. Check the network, then retry once."
        } else if error.contains("too large") {
            "OpenCode Zen's response exceeded Damon's fixed size bound. Select a less verbose model."
        } else {
            "OpenCode Zen answered, but the response was invalid or the meaning failed Damon's checks. Select another model or rephrase the request."
        }
    } else if !error.contains("Ollama:") {
        "No usable language model response was available. Select another configured model."
    } else if error.contains("cannot connect")
        || error.contains("cannot resolve")
        || error.contains("timed out")
    {
        "I couldn't reach Ollama. Check the Ollama connection and selected model."
    } else if error.contains("exceeds 8 MiB") || error.contains("exceeds 256 KiB") {
        "Ollama's response exceeded Damon's safety limit. Try a smaller or less verbose model."
    } else if error.contains("HTTP") || error.contains("stream error") {
        "Ollama stopped before returning a complete meaning. Try the request again or select another model."
    } else {
        "Ollama answered, but its meaning did not pass Damon's safety checks. Try rephrasing the request or select a stronger model."
    };
    format!("I don't know that request deterministically yet. {detail} Try rephrasing it, or type “help” to see my current native capabilities.")
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

#[cfg(test)]
mod policy_confirmation_tests {
    use super::*;

    #[test]
    fn observed_zen_errors_are_not_reported_as_invalid_meaning() {
        let restricted = teacher_unavailable(
            "OpenCode Zen model request failed (HTTP 400): Error from provider (Console): OpenCode's free tier can only be used in OpenCode",
        );
        assert!(restricted.contains("restricts the selected free model"));
        assert!(!restricted.contains("meaning failed"));

        let unavailable = teacher_unavailable(
            "OpenCode Zen model request failed (HTTP 500): Error from provider (Console): Upstream request failed: Model is unavailable.",
        );
        assert!(unavailable.contains("currently unavailable"));
        assert!(!unavailable.contains("meaning failed"));
    }

    #[test]
    fn explicit_confirmation_executes_and_same_exact_action_never_asks_again() {
        let directory =
            std::env::temp_dir().join(format!("damon-policy-confirmation-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir(&directory).unwrap();
        let mut policy = Policy::default();
        policy.allowed = crate::types::Effects::READ;
        let mut damon = Damon {
            data: DamonData::open(directory.join("brain.data")).unwrap(),
            models: ModelRouter::default(),
            policy,
        };

        let first = damon.handle("what is my default gateway?");
        assert!(first.contains("process access"));
        assert!(first.contains("do it"));

        let confirmed = damon.handle("do it");
        assert!(!confirmed.contains("Policy blocked"));
        let repeated = damon.handle("what is my default gateway?");
        assert!(!repeated.contains("Policy blocked"));
        assert_eq!(damon.data.approvals.grants.len(), 1);

        drop(damon);
        let mut reopened = Damon::open(directory.join("brain.data")).unwrap();
        reopened.policy.allowed = crate::types::Effects::READ;
        let after_restart = reopened.handle("what is my default gateway?");
        assert!(!after_restart.contains("Policy blocked"));
        assert_eq!(
            reopened.handle("forget my approvals"),
            "I revoked every remembered policy approval."
        );
        let after_revocation = reopened.handle("what is my default gateway?");
        assert!(after_revocation.contains("Policy blocked"));
        drop(reopened);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
