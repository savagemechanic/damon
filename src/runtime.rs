use std::env;
use std::io;
use std::path::PathBuf;

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
                            Ok(m) => m,
                            Err(e) => return format!("I don't know how to do that yet. {e}"),
                        }
                    }
                } else {
                    return "I don't know how to do that yet.".into();
                }
            }
            Interpretation::Unknown { .. } => match self.ask_teacher(input) {
                Ok(m) => m,
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
            let result = tools::execute(action, &self.policy);
            outcomes.push(result.success);
            messages.push(render_result(result));
        }
        learning::observe_verified(
            &mut self.data,
            feature,
            &meaning,
            outcomes.iter().all(|v| *v),
        );
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
                "Memory generation {}: {} entities, {} learned phrases, {} retained experiences, and {} cached computations.",
                self.data.generation(),
                self.data.entities.len(),
                self.data.language_counts.len(),
                self.data.experiences.len(),
                self.data.memo.entries.len()
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

    fn ask_teacher(&self, input: &str) -> Result<MeaningGraph, String> {
        let prompt = crate::semantics::teacher_prompt(input, &self.data);
        let response = self.models.infer_validated(&prompt, |text| {
            crate::semantics::parse_teacher(text, &self.data)
                .and_then(|m| crate::semantics::validate_request(input, &m, &self.data))
        })?;
        crate::semantics::parse_teacher(&response.text, &self.data)
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
