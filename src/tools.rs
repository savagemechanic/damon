use crate::data::DamonData;
use crate::language::{INTENT_GIT_DIFF, INTENT_GIT_STATUS, INTENT_LIST_FILES, INTENT_RUN_TESTS};
use crate::types::{Action, Effects, MeaningGraph, ToolId, ToolResult};
use std::path::Path;
use std::time::Duration;

pub const TOOL_GIT_STATUS: ToolId = ToolId(1);
pub const TOOL_GIT_DIFF: ToolId = ToolId(2);
pub const TOOL_TEST: ToolId = ToolId(3);
pub const TOOL_LIST_FILES: ToolId = ToolId(4);
pub fn action_from_meaning(m: &MeaningGraph, data: &DamonData) -> Result<Action, String> {
    let cwd = m
        .target
        .and_then(|id| data.entity(id))
        .map(|e| e.value.clone())
        .unwrap_or_else(|| ".".into());
    let (tool, effects, args) = if m.intent == INTENT_GIT_STATUS {
        (
            TOOL_GIT_STATUS,
            Effects::READ.union(Effects::PROCESS),
            vec![cwd],
        )
    } else if m.intent == INTENT_GIT_DIFF {
        (
            TOOL_GIT_DIFF,
            Effects::READ.union(Effects::PROCESS),
            vec![cwd],
        )
    } else if m.intent == INTENT_RUN_TESTS {
        (TOOL_TEST, Effects::READ.union(Effects::PROCESS), vec![cwd])
    } else if m.intent == INTENT_LIST_FILES {
        (TOOL_LIST_FILES, Effects::READ, vec![cwd])
    } else {
        return Err("meaning does not map to a deterministic tool".into());
    };
    Ok(Action {
        tool,
        target: m.target,
        effects,
        args,
    })
}
pub fn execute(action: &Action) -> ToolResult {
    let cwd = action.args.first().map(String::as_str).unwrap_or(".");
    match action.tool {
        TOOL_GIT_STATUS => run(cwd, "git", &["status", "--short", "--branch"]),
        TOOL_GIT_DIFF => run(cwd, "git", &["diff", "--stat", "--", "."]),
        TOOL_TEST => run_tests(cwd),
        TOOL_LIST_FILES => list_files(cwd),
        _ => ToolResult {
            success: false,
            stdout: String::new(),
            stderr: "unknown tool".into(),
            code: None,
        },
    }
}
fn run(cwd: &str, program: &str, args: &[&str]) -> ToolResult {
    crate::process::run(
        Path::new(cwd),
        program,
        args,
        None,
        Duration::from_secs(120),
    )
}

fn run_tests(cwd: &str) -> ToolResult {
    let p = Path::new(cwd);
    if p.join("Cargo.toml").exists() {
        return run(cwd, "cargo", &["test", "--quiet"]);
    }
    if p.join("pyproject.toml").exists() || p.join("pytest.ini").exists() {
        return run(cwd, "python3", &["-m", "pytest", "-q"]);
    }
    if p.join("package.json").exists() {
        return run(cwd, "npm", &["test", "--", "--runInBand"]);
    }
    ToolResult {
        success: false,
        stdout: String::new(),
        stderr: "no supported test runner detected".into(),
        code: None,
    }
}
fn list_files(cwd: &str) -> ToolResult {
    let mut names = Vec::new();
    match std::fs::read_dir(cwd) {
        Ok(rd) => {
            for entry in rd.take(200) {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        return ToolResult {
                            success: false,
                            stdout: String::new(),
                            stderr: e.to_string(),
                            code: None,
                        }
                    }
                };
                names.push(entry.file_name().to_string_lossy().to_string());
            }
            names.sort();
            ToolResult {
                success: true,
                stdout: names.join("\n"),
                stderr: String::new(),
                code: Some(0),
            }
        }
        Err(e) => ToolResult {
            success: false,
            stdout: String::new(),
            stderr: e.to_string(),
            code: None,
        },
    }
}
