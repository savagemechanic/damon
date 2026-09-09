//! Structured execution truth. Rendering is a separate deterministic step.
use crate::types::{Action, ToolResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Success,
    Failure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub kind: &'static str,
    pub passed: u32,
    pub failed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub files_modified: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub stage: &'static str,
    pub message: String,
    pub code: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultIr {
    pub status: Status,
    pub observations: Vec<String>,
    pub changes: Vec<Change>,
    pub checks: Vec<Check>,
    pub artifacts: Vec<Artifact>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ResultIr {
    pub fn from_tool(action: &Action, result: &ToolResult) -> Self {
        let mut ir = Self {
            status: if result.success {
                Status::Success
            } else {
                Status::Failure
            },
            observations: Vec::new(),
            changes: Vec::new(),
            checks: Vec::new(),
            artifacts: Vec::new(),
            diagnostics: Vec::new(),
        };
        if action.capability == crate::capability::RUN_TESTS {
            if let Some(check) = parse_test_counts(&result.stdout) {
                ir.checks.push(check);
            }
        }
        let stdout = result.stdout.trim();
        if !stdout.is_empty() && ir.checks.is_empty() {
            ir.observations.push(stdout.to_string());
        }
        let stderr = result.stderr.trim();
        if !result.success {
            ir.diagnostics.push(Diagnostic {
                stage: "execution",
                message: if stderr.is_empty() {
                    "operation failed without a diagnostic".into()
                } else {
                    stderr.to_string()
                },
                code: result.code,
            });
        }
        ir
    }

    pub fn render_english(&self) -> String {
        if self.status == Status::Failure {
            let Some(diagnostic) = self.diagnostics.first() else {
                return "The operation failed.".into();
            };
            return format!(
                "The operation failed{}: {}",
                diagnostic
                    .code
                    .map(|code| format!(" with exit code {code}"))
                    .unwrap_or_default(),
                diagnostic.message
            );
        }
        let mut sentences = Vec::new();
        for change in &self.changes {
            sentences.push(match change.files_modified {
                1 => "One file changed.".into(),
                count => format!("{count} files changed."),
            });
        }
        for check in &self.checks {
            sentences.push(if check.failed == 0 {
                match check.passed {
                    1 => format!("The {} passed.", check.kind),
                    passed => format!("All {passed} {} passed.", check.kind),
                }
            } else {
                format!(
                    "{} {} passed and {} failed.",
                    check.passed, check.kind, check.failed
                )
            });
        }
        sentences.extend(self.observations.iter().cloned());
        if sentences.is_empty() {
            "Done. The operation completed successfully.".into()
        } else {
            sentences.join("\n")
        }
    }
}

fn parse_test_counts(output: &str) -> Option<Check> {
    // Cargo's stable summary: "test result: ok. 84 passed; 0 failed; ..."
    let cargo_summaries = output
        .lines()
        .filter(|line| line.contains("test result:") && line.contains(" passed; "))
        .collect::<Vec<_>>();
    if !cargo_summaries.is_empty() {
        let passed = cargo_summaries
            .iter()
            .filter_map(|summary| number_before(summary, " passed"))
            .sum();
        let failed = cargo_summaries
            .iter()
            .filter_map(|summary| number_before(summary, " failed"))
            .sum();
        return Some(Check {
            kind: "tests",
            passed,
            failed,
        });
    }
    // Pytest's compact summary: "84 passed" or "83 passed, 1 failed".
    let summary = output.lines().rev().find(|line| line.contains("passed"))?;
    let passed = number_before(summary, " passed")?;
    let failed = number_before(summary, " failed").unwrap_or(0);
    Some(Check {
        kind: "tests",
        passed,
        failed,
    })
}

fn number_before(text: &str, marker: &str) -> Option<u32> {
    let prefix = text.split_once(marker)?.0;
    prefix
        .split(|character: char| !character.is_ascii_digit())
        .rfind(|part| !part.is_empty())?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_test_facts_without_a_model() {
        let action = Action {
            capability: crate::capability::RUN_TESTS,
            tool: crate::tools::TOOL_TEST,
            implementation_version: 1,
            target: None,
            effects: crate::types::Effects::READ.union(crate::types::Effects::PROCESS),
            args: Vec::new(),
        };
        let result = ToolResult {
            success: true,
            stdout: "test result: ok. 84 passed; 0 failed; 0 ignored".into(),
            stderr: String::new(),
            code: Some(0),
        };
        let ir = ResultIr::from_tool(&action, &result);
        assert_eq!(ir.checks[0].passed, 84);
        assert_eq!(ir.render_english(), "All 84 tests passed.");
    }

    #[test]
    fn combines_changes_and_checks_from_structured_facts() {
        let ir = ResultIr {
            status: Status::Success,
            observations: Vec::new(),
            changes: vec![Change { files_modified: 3 }],
            checks: vec![Check {
                kind: "tests",
                passed: 84,
                failed: 0,
            }],
            artifacts: Vec::new(),
            diagnostics: Vec::new(),
        };
        assert_eq!(
            ir.render_english(),
            "3 files changed.\nAll 84 tests passed."
        );
    }
}
